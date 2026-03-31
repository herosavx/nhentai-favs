use anyhow::Result;
use rusqlite::{params, Connection};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::models::{FavItem, TagItem};

pub struct Database {
    favs_conn: Connection,
    pub outpath: PathBuf,
}

impl Database {
    pub fn new(outpath: &Path) -> Result<Self> {
        let favs_db_path = outpath.join("nfavs.db");
        let favs_conn = Connection::open(&favs_db_path)?;

        favs_conn.execute(
            "CREATE TABLE IF NOT EXISTS favorites (
                local_id INTEGER PRIMARY KEY AUTOINCREMENT,
                nhen_id INTEGER UNIQUE,
                english_title TEXT,
                japanese_title TEXT,
                num_pages INTEGER,
                thumbnail TEXT,
                tag_ids TEXT
            )",
            [],
        )?;

        Ok(Self {
            favs_conn,
            outpath: outpath.to_path_buf(),
        })
    }

    pub fn fav_exists(&self, nhen_id: u32) -> Result<bool> {
        let mut stmt = self.favs_conn.prepare("SELECT 1 FROM favorites WHERE nhen_id = ?1")?;
        Ok(stmt.exists(params![nhen_id])?)
    }

    pub fn insert_fav(&self, fav: &FavItem) -> Result<()> {
        let tag_ids_str = fav.tag_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
        
        self.favs_conn.execute(
            "INSERT INTO favorites (nhen_id, english_title, japanese_title, num_pages, thumbnail, tag_ids)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                fav.id,
                fav.english_title.as_deref().unwrap_or(""),
                fav.japanese_title.as_deref().unwrap_or(""),
                fav.num_pages,
                fav.thumbnail,
                tag_ids_str
            ],
        )?;
        Ok(())
    }

    pub fn get_all_thumbnails(&self) -> Result<Vec<(u32, String)>> {
        let mut stmt = self.favs_conn.prepare("SELECT nhen_id, thumbnail FROM favorites")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub fn export_to_csv(&self) -> Result<()> {
        let csv_path = self.outpath.join("nfavs_export.csv");
        let mut stmt = self.favs_conn.prepare(
            "SELECT local_id, nhen_id, english_title, japanese_title, num_pages 
             FROM favorites ORDER BY local_id DESC"
        )?;

        let mut wtr = csv::Writer::from_path(&csv_path)?;
        wtr.write_record(&["local_id", "nhen_id", "english_title", "japanese_title", "num_pages"])?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;

        for row_result in rows {
            let (lid, nid, eng, jap, pages) = row_result?;
            wtr.write_record(&[
                lid.to_string(), nid.to_string(), eng, jap, pages.to_string()
            ])?;
        }

        wtr.flush()?;
        println!("[+] Exported database to {}", csv_path.display());
        Ok(())
    }

    pub fn count_favs(&self) -> Result<u32> {
        let mut stmt = self.favs_conn.prepare("SELECT COUNT(*) FROM favorites")?;
        let count: u32 = stmt.query_row([], |row| row.get(0))?;
        Ok(count)
    }

    pub fn backup_nfavs(&self) -> Result<()> {
        let db_path = self.outpath.join("nfavs.db");
        if db_path.exists() {
            let prev_dir = self.outpath.join(".prevstate");
            fs::create_dir_all(&prev_dir)?;
            fs::copy(&db_path, prev_dir.join("nfavs.db"))?;
        }
        Ok(())
    }

    pub fn restore_nfavs(&self) -> Result<()> {
        let db_path = self.outpath.join("nfavs.db");
        let backup_path = self.outpath.join(".prevstate").join("nfavs.db");

        if !backup_path.exists() {
            println!("[-] No previous nfavs database found in .prevstate folder.");
            return Ok(());
        }

        // Check the count in the backup database
        let backup_conn = Connection::open(&backup_path)?;
        let mut backup_stmt = backup_conn.prepare("SELECT COUNT(*) FROM favorites")?;
        let prev_count: u32 = backup_stmt.query_row([], |row| row.get(0)).unwrap_or(0);
        
        println!("[*] Previous database contains {} books", prev_count);

        if db_path.exists() {
            let current_count = self.count_favs().unwrap_or(0);
            println!("[*] Current database contains {} books", current_count);
            println!("[!] WARNING: Restoring will replace the current database");
        } else {
            println!("[*] No current database exists");
        }

        print!("\n[?] Do you want to restore from the previous database? (y/N): ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if input.trim().eq_ignore_ascii_case("y") {
            fs::copy(&backup_path, &db_path)?;
            println!("[+] Successfully restored nfavs.db from previous state.");
        } else {
            println!("[*] Restore cancelled");
        }

        Ok(())
    }
}

pub struct TagsDatabase {
    conn: Connection,
}

impl TagsDatabase {
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY,
                type TEXT,
                name TEXT,
                count INTEGER
            )",
            [],
        )?;
        Ok(Self { conn })
    }

    pub fn insert_tag(&self, tag: &TagItem) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO tags (id, type, name, count) VALUES (?1, ?2, ?3, ?4)",
            params![tag.id, tag.tag_type, tag.name, tag.count],
        )?;
        Ok(())
    }
}
