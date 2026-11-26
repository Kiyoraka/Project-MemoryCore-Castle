//! When Worldsmith refuses a game: an anti-cheat module is loaded, or the game is talking to a
//! remote server. Pure rules; the OS side gathers module names and remote addresses.

use std::net::IpAddr;

/// Lower-case substrings of a module (DLL / exe) name, and the anti-cheat each one means.
/// Valve's VAC can't be seen from user mode, so it isn't listed.
const ANTI_CHEAT: &[(&str, &str)] = &[
    ("easyanticheat", "EasyAntiCheat"),
    ("eac_", "EasyAntiCheat"),
    ("beclient", "BattlEye"),
    ("battleye", "BattlEye"),
    ("vgk", "Riot Vanguard"),
    ("vanguard", "Riot Vanguard"),
    ("gameguard", "nProtect GameGuard"),
    ("npggnt", "nProtect GameGuard"),
    ("xigncode", "XIGNCODE3"),
    ("x3.xem", "XIGNCODE3"),
    // Epic Online Services anti-cheat, and any other module that names itself anti-cheat
    ("eosanticheat", "EOS Anti-Cheat"),
    ("anticheat", "anti-cheat"),
    ("anti-cheat", "anti-cheat"),
];

/// The anti-cheat a module name points to, if any (case-insensitive).
pub fn anti_cheat_in(module: &str) -> Option<&'static str> {
    let m = module.to_ascii_lowercase();
    if m.contains("eossdk") && m.contains("anticheat") {
        return Some("EOS Anti-Cheat");
    }
    ANTI_CHEAT.iter().find(|(needle, _)| m.contains(needle)).map(|(_, name)| *name)
}

/// Is a remote address a real network peer (not this PC)?
pub fn is_online_peer(remote: IpAddr) -> bool {
    match remote {
        IpAddr::V4(v4) => !(v4.is_loopback() || v4.is_unspecified()),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_online_peer(IpAddr::V4(v4));
            }
            !(v6.is_loopback() || v6.is_unspecified())
        }
    }
}

/// The refusal line for a game, or None when Worldsmith may attach.
/// `modules` are the loaded module names; `remotes` the remote ends of its ESTABLISHED TCP links.
pub fn check(modules: &[String], remotes: &[IpAddr]) -> Option<String> {
    if let Some(ac) = modules.iter().find_map(|m| anti_cheat_in(m)) {
        return Some(format!(
            "Worldsmith only works with offline single-player games - this one runs anti-cheat ({ac})."
        ));
    }
    if remotes.iter().any(|r| is_online_peer(*r)) {
        return Some(
            "Worldsmith only works with offline single-player games - this one is connected to an online server."
                .to_string(),
        );
    }
    None
}

/// The refusal when the module list couldn't be read at all.
pub fn unreadable() -> String {
    "Worldsmith couldn't check this game for anti-cheat, so it won't touch it.".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn mods(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn anti_cheat_modules_refuse() {
        for (name, ac) in [
            ("EasyAntiCheat_x64.dll", "EasyAntiCheat"),
            ("EAC_Launcher.dll", "EasyAntiCheat"),
            ("BEClient_x64.dll", "BattlEye"),
            ("BattlEye.dll", "BattlEye"),
            ("vgk.sys", "Riot Vanguard"),
            ("GameGuard.des", "nProtect GameGuard"),
            ("npggNT.des", "nProtect GameGuard"),
            ("xigncode.dll", "XIGNCODE3"),
            ("x3.xem", "XIGNCODE3"),
            ("EOSSDK-Win64-AntiCheat.dll", "EOS Anti-Cheat"),
        ] {
            let r = check(&mods(&["kernel32.dll", name]), &[]).expect(name);
            assert!(r.contains(ac), "{name}: {r}");
            assert!(r.starts_with("Worldsmith only works with offline single-player games"));
        }
    }

    #[test]
    fn ordinary_games_pass() {
        let m = mods(&["Game.exe", "kernel32.dll", "d3d11.dll", "EOSSDK-Win64-Shipping.dll", "steam_api64.dll"]);
        assert_eq!(check(&m, &[]), None);
    }

    #[test]
    fn loopback_is_offline_but_a_remote_peer_is_not() {
        let m = mods(&["Game.exe"]);
        let lo: Vec<IpAddr> = vec![
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped()),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        ];
        assert_eq!(check(&m, &lo), None);
        let online = check(&m, &[IpAddr::V4(Ipv4Addr::new(52, 1, 2, 3))]).unwrap();
        assert!(online.contains("online"));
        assert!(check(&m, &[IpAddr::V6(Ipv4Addr::new(8, 8, 8, 8).to_ipv6_mapped())]).is_some());
    }
}
