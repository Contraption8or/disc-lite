//! Adding folders under other folders, and re-linking a folder to a
//! different directory — the settings side, kept apart from the dialogs so
//! it can be tested.

use crate::model::{Folder, folder_name_from_path, new_folder_id};
use crate::persist::Settings;
use crate::tree::Model;

/// What `nest_paths` did.
#[derive(Debug, Default, PartialEq)]
pub struct Nested {
    /// Ids of folders newly added to Disc.
    pub added: Vec<String>,
    /// How many already-added folders were moved under the parent instead.
    pub moved: usize,
}

fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    norm(a) == norm(b)
}

/// Adds `paths` to Disc nested under `parent` (a tree node key; "" = top
/// level). A path that's already in Disc is moved there instead of being
/// added twice — unless that would put a folder inside itself. Writes the
/// same placement/order data that dragging in the tree does.
pub fn nest_paths(settings: &mut Settings, model: &Model, paths: Vec<String>, parent: &str) -> Nested {
    let mut fresh: Vec<Folder> = Vec::new();
    let mut placed: Vec<String> = Vec::new();
    let mut moved = 0;
    for path in paths {
        if let Some(f) = settings.folders.iter().find(|f| same_path(&f.folder_path, &path)) {
            let into_itself = !parent.is_empty() && model.is_within(parent, &f.id);
            if !into_itself && !placed.contains(&f.id) {
                placed.push(f.id.clone());
                moved += 1;
            }
            continue;
        }
        if fresh.iter().any(|f| same_path(&f.folder_path, &path)) {
            continue;
        }
        let id = format!("{}-{}", new_folder_id(), fresh.len());
        fresh.push(Folder { id: id.clone(), name: folder_name_from_path(&path), folder_path: path });
        placed.push(id);
    }
    fresh.sort_by_key(|f| f.name.to_lowercase());
    let added: Vec<String> = fresh.iter().map(|f| f.id.clone()).collect();
    settings.folders.extend(fresh);

    for id in &placed {
        for list in settings.order.values_mut() {
            list.retain(|k| k != id);
        }
        if parent.is_empty() {
            settings.placement.remove(id);
        } else {
            settings.placement.insert(id.clone(), parent.to_string());
        }
        // Only extend an explicit order; otherwise it just sorts
        // alphabetically among its new siblings.
        if let Some(list) = settings.order.get_mut(parent) {
            list.push(id.clone());
        }
    }
    Nested { added, moved }
}

/// Points folder `id` at `path`. Ok(false) if it already pointed there;
/// Err with a message if another Disc folder already uses that path. The
/// name follows the new directory unless it had been renamed in Disc.
pub fn relink(settings: &mut Settings, id: &str, path: &str) -> Result<bool, String> {
    let Some(current) = settings.folders.iter().find(|f| f.id == id) else {
        return Err("That folder is no longer in Disc".into());
    };
    if same_path(&current.folder_path, path) {
        return Ok(false);
    }
    if let Some(other) = settings.folders.iter().find(|f| f.id != id && same_path(&f.folder_path, path)) {
        return Err(format!("That folder is already in Disc as \"{}\"", other.name));
    }
    let f = settings.folders.iter_mut().find(|f| f.id == id).unwrap();
    if f.name == folder_name_from_path(&f.folder_path) {
        f.name = folder_name_from_path(path);
    }
    f.folder_path = path.to_string();
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::build_model;
    use std::collections::HashMap;

    fn folder(id: &str, path: &str) -> Folder {
        Folder { id: id.into(), name: folder_name_from_path(path), folder_path: path.into() }
    }

    fn setup() -> Settings {
        Settings {
            folders: vec![folder("a", r"E:\Music\Alpha"), folder("b", r"E:\Music\Beta")],
            ..Settings::default()
        }
    }

    fn model(s: &Settings) -> Model {
        build_model(&s.folders, &HashMap::new(), &s.placement, &s.order)
    }

    #[test]
    fn adds_new_folder_inside_parent() {
        let mut s = setup();
        let m = model(&s);
        let r = nest_paths(&mut s, &m, vec![r"D:\Sfx\Whooshes".into()], "a");
        assert_eq!(r.added.len(), 1);
        assert_eq!(r.moved, 0);
        let id = &r.added[0];
        let f = s.folders.iter().find(|f| &f.id == id).unwrap();
        assert_eq!(f.name, "Whooshes");
        assert_eq!(s.placement.get(id).map(String::as_str), Some("a"));
        // And the tree really shows it under Alpha.
        let m = model(&s);
        assert!(m.children("a").contains(id));
        assert!(!m.children("").contains(id));
    }

    #[test]
    fn existing_folder_is_moved_not_duplicated() {
        let mut s = setup();
        let m = model(&s);
        // Same path, different case / trailing slash.
        let r = nest_paths(&mut s, &m, vec![r"e:\music\beta\".into()], "a");
        assert_eq!(r, Nested { added: vec![], moved: 1 });
        assert_eq!(s.folders.len(), 2);
        assert_eq!(s.placement.get("b").map(String::as_str), Some("a"));
    }

    #[test]
    fn refuses_to_nest_a_folder_inside_itself() {
        let mut s = setup();
        s.placement.insert("b".into(), "a".into()); // Beta is inside Alpha
        let m = model(&s);
        let r = nest_paths(&mut s, &m, vec![r"E:\Music\Alpha".into()], "b");
        assert_eq!(r, Nested::default());
        assert!(!s.placement.contains_key("a"));
    }

    #[test]
    fn duplicates_in_one_pick_are_added_once_and_top_level_works() {
        let mut s = setup();
        let m = model(&s);
        let r = nest_paths(&mut s, &m, vec![r"D:\New".into(), r"D:\new\".into()], "");
        assert_eq!(r.added.len(), 1);
        assert!(!s.placement.contains_key(&r.added[0]));
        assert!(model(&s).children("").contains(&r.added[0]));
    }

    #[test]
    fn appends_to_an_explicit_order_only() {
        let mut s = setup();
        s.order.insert("a".into(), vec!["x".into()]);
        let m = model(&s);
        let r = nest_paths(&mut s, &m, vec![r"D:\One".into()], "a");
        assert_eq!(s.order["a"], vec!["x".to_string(), r.added[0].clone()]);
        let r2 = nest_paths(&mut s, &m, vec![r"D:\Two".into()], "b");
        assert!(!s.order.contains_key("b"));
        assert_eq!(s.placement.get(&r2.added[0]).map(String::as_str), Some("b"));
    }

    #[test]
    fn relink_follows_name_unless_renamed() {
        let mut s = setup();
        assert_eq!(relink(&mut s, "a", r"F:\Library\Gamma"), Ok(true));
        assert_eq!(s.folders[0].name, "Gamma");
        assert_eq!(s.folders[0].folder_path, r"F:\Library\Gamma");

        s.folders[1].name = "My Beta".into();
        assert_eq!(relink(&mut s, "b", r"F:\Library\Delta"), Ok(true));
        assert_eq!(s.folders[1].name, "My Beta");

        assert_eq!(relink(&mut s, "b", r"f:\library\delta\"), Ok(false));
        assert!(relink(&mut s, "b", r"F:\Library\Gamma").unwrap_err().contains("Gamma"));
    }
}
