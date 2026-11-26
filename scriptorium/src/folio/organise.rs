//! Organising pages: deleting pages (each taken out of its parent's /Kids, the /Count of every node
//! above it lowered) and rotating them (the page's own /Rotate). Both are ordinary edits in the
//! overlay, so the save appends them as an incremental update; the page objects themselves stay in
//! the file (nothing unknown is lost, and an undo puts them back).

use super::obj::Obj;
use super::FolioDoc;

impl FolioDoc {
    /// Pages from a list, checked: each exists, none twice, ascending.
    fn checked_pages(&self, pages: &[usize]) -> Result<Vec<usize>, String> {
        let n = self.pages.len();
        let mut v = pages.to_vec();
        v.sort_unstable();
        v.dedup();
        if v.is_empty() {
            return Err("name the pages (numbered from 0)".into());
        }
        if let Some(&bad) = v.iter().find(|&&p| p >= n) {
            return Err(format!("no page {} (the document has {} pages, numbered from 0)", bad, n));
        }
        for &p in &v {
            if self.pages[p].num == 0 {
                return Err(format!("page {} cannot be changed (its dictionary is not an object of its own)", p));
            }
        }
        Ok(v)
    }

    /// Delete pages (numbered from 0). At least one page must stay.
    pub fn delete_pages(&mut self, pages: &[usize]) -> Result<(), String> {
        let v = self.checked_pages(pages)?;
        if v.len() >= self.pages.len() {
            return Err("a PDF keeps at least one page; delete fewer pages".into());
        }
        let nums: Vec<u32> = v.iter().map(|&p| self.pages[p].num).collect();
        for num in nums {
            let page = self.file.get(num);
            let parent = page.as_dict().and_then(|d| d.get("Parent")).and_then(Obj::as_ref).map(|r| r.0).ok_or("a page has no parent in the page tree")?;
            let pd = self.file.get(parent).as_dict().cloned().ok_or("the page tree is damaged")?;
            // The /Kids may be an array object of its own.
            match pd.get("Kids") {
                Some(Obj::Ref(k, _)) => {
                    let k = *k;
                    let mut kids = self.file.get(k).as_arr().map(|a| a.to_vec()).unwrap_or_default();
                    kids.retain(|o| o.as_ref().map(|r| r.0) != Some(num));
                    self.set_object(k, Obj::Arr(kids));
                }
                Some(Obj::Arr(a)) => {
                    let mut kids = a.clone();
                    kids.retain(|o| o.as_ref().map(|r| r.0) != Some(num));
                    let mut d = pd.clone();
                    d.set("Kids", Obj::Arr(kids));
                    self.set_object(parent, Obj::Dict(d));
                }
                _ => return Err("the page tree is damaged".into()),
            }
            // Every node from the parent up counts one page fewer.
            let mut node = Some(parent);
            let mut guard = 0;
            while let Some(n) = node {
                guard += 1;
                if guard > 64 {
                    break;
                }
                let Some(mut d) = self.file.get(n).as_dict().cloned() else { break };
                let count = self.file.dget(&d, "Count").as_i64().unwrap_or(1);
                d.set("Count", Obj::Int((count - 1).max(0)));
                node = d.get("Parent").and_then(Obj::as_ref).map(|r| r.0);
                self.set_object(n, Obj::Dict(d));
            }
        }
        self.reload_structure();
        self.render_all();
        Ok(())
    }

    /// Turn pages clockwise by `angle` (90, 180 or 270; a negative angle turns them back).
    pub fn rotate_pages(&mut self, pages: &[usize], angle: i64) -> Result<(), String> {
        if angle % 90 != 0 {
            return Err("pages turn by 90, 180 or 270 degrees".into());
        }
        let v = self.checked_pages(pages)?;
        for &p in &v {
            let info = &self.pages[p];
            let num = info.num;
            let new = ((info.rotate as i64 + angle) % 360 + 360) % 360;
            let mut d = self.file.get(num).as_dict().cloned().ok_or("the page object is missing")?;
            d.set("Rotate", Obj::Int(new));
            self.set_object(num, Obj::Dict(d));
        }
        self.reload_structure();
        for p in v {
            self.rerender(p);
        }
        Ok(())
    }
}
