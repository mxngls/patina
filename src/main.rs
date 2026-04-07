use std::path::Path;

use anyhow::{Context, Result};

use rusqlite::{Connection, OpenFlags};

const SAFARI_DIR: &str = "Library/Containers/com.apple.Safari";
const SAFARI_DATA_DIR: &str = "Data/Library/Safari";
const SAFARI_TABS_DB: &str = "SafariTabs.db";

#[derive(Debug)]
struct TabGroup {
    title: String,
    tabs: Vec<(String, String)>,
}

fn get_tabs(db_path: &Path) -> Result<Vec<TabGroup>> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("Failed to open Safari tabs database");

    let mut stmt = conn
        .prepare(
            "
    SELECT
        parent.title as group_title,
        child.title as tab_title,
        child.url as tab_url
    FROM bookmarks parent
    JOIN bookmarks child ON child.parent = parent.id
    WHERE parent.type = 1
    AND parent.parent = 0
    AND parent.subtype = 0
    AND parent.num_children > 0
    AND parent.hidden = 0
    AND child.title NOT IN (
        'TopScopedBookmarkList',
        'Untitled',
        'Start Page'
    )
    ORDER BY parent.id DESC, child.order_index ASC
",
        )
        .expect("Failed to prepare statement");

    let mut tab_groups: Vec<TabGroup> = Vec::new();

    stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>("group_title")?,
            row.get::<_, String>("tab_title")?,
            row.get::<_, String>("tab_url")?,
        ))
    })
    .context("Failed to execute query")?
    .filter_map(|r| match r {
        Ok(r) => Some(r),
        Err(e) => {
            eprintln!("Skipping malformed row: {e}");
            None
        }
    })
    .fold(
        &mut tab_groups,
        |groups, (group_title, tab_title, tab_url)| {
            match groups.last_mut() {
                Some(group) if group.title == group_title => {
                    group.tabs.push((tab_title, tab_url));
                }
                _ => {
                    groups.push(TabGroup {
                        title: group_title,
                        tabs: vec![(tab_title, tab_url)],
                    });
                }
            }
            groups
        },
    );

    Ok(tab_groups)
}

fn main() -> Result<()> {
    let db = std::env::home_dir()
        .context("HOME to be set.")?
        .join(SAFARI_DIR)
        .join(SAFARI_DATA_DIR)
        .join(SAFARI_TABS_DB);

    if !db.is_file() {
        return Err(anyhow::anyhow!(
            "Safari tabs database does not exist at \"{}\"",
            db.to_string_lossy()
        ));
    }

    let tabs = get_tabs(&db)?;

    println!("{tabs:#?}");

    Ok(())
}
