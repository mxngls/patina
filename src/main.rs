use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result};
use plist::Value;
use rusqlite::{Connection, OpenFlags};

const SAFARI_DIR: &str = "Library/Containers/com.apple.Safari";
const SAFARI_DATA_DIR: &str = "Data/Library/Safari";
const SAFARI_TABS_DB: &str = "SafariTabs.db";

#[derive(Debug)]
struct Tab {
    title: String,
    url: String,
    date_added: Option<String>,
    date_last_viewed: Option<String>,
}

#[derive(Debug)]
struct TabGroup {
    title: String,
    tabs: Vec<Tab>,
}

fn parse_extra_attributes(blob: &[u8]) -> (Option<String>, Option<String>) {
    let Ok(Value::Dictionary(dict)) = Value::from_reader(Cursor::new(blob)) else {
        return (None, None);
    };

    let date_added = dict
        .get("com.apple.Bookmark")
        .and_then(|v| v.as_dictionary())
        .and_then(|d| d.get("DateAdded"))
        .and_then(plist::Value::as_date)
        .map(|d| d.to_xml_format());

    let date_last_viewed = dict
        .get("DateLastViewed")
        .and_then(plist::Value::as_date)
        .map(|d| d.to_xml_format());

    (date_added, date_last_viewed)
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
        child.url as tab_url,
        child.extra_attributes
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

    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>("group_title")?,
                row.get::<_, String>("tab_title")?,
                row.get::<_, String>("tab_url")?,
                row.get::<_, Option<Vec<u8>>>("extra_attributes")?,
            ))
        })
        .context("Failed to execute query")?;

    Ok(rows
        .map(|r| {
            let (group_title, title, url, extra) = r?;
            let (date_added, date_last_viewed) = extra
                .as_deref()
                .map_or((None, None), parse_extra_attributes);
            Ok((
                group_title,
                Tab {
                    title,
                    url,
                    date_added,
                    date_last_viewed,
                },
            ))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .fold(Vec::new(), |mut groups, (group_title, tab)| {
            match groups.last_mut() {
                Some(group) if group.title == group_title => {
                    group.tabs.push(tab);
                }
                _ => {
                    groups.push(TabGroup {
                        title: group_title,
                        tabs: vec![tab],
                    });
                }
            }
            groups
        }))
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

    println!("{:#?}", tabs.first());

    Ok(())
}
