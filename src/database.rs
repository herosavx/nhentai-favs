use crate::error::Result;
use crate::models::BookData;
use rusqlite::{Connection, params};
use std::path::Path;

pub const DB_FILENAME: &str = "nfavs.db";

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        Ok(Self { conn })
    }

    pub fn initialize(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS books (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                nhen_id TEXT UNIQUE,
                title_1 TEXT,
                title_2 TEXT,
                artists TEXT,
                groups TEXT,
                tags TEXT,
                languages TEXT,
                pages INTEGER,
                image_ext TEXT
            )",
            [],
        )?;
        Ok(())
    }

    pub fn book_exists(&self, id: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT nhen_id FROM books WHERE nhen_id = ?1")?;
        Ok(stmt.exists(params![id])?)
    }

    pub fn save_book(&self, book_data: &BookData, ext: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO books 
             (nhen_id, title_1, title_2, artists, groups, tags, languages, pages, image_ext)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                book_data.id,
                book_data.title_1,
                book_data.title_2,
                book_data.artists,
                book_data.groups,
                book_data.tags,
                book_data.languages,
                book_data.pages,
                ext,
            ],
        )?;
        Ok(())
    }

    pub fn count_books(&self) -> Result<u32> {
        let mut stmt = self.conn.prepare("SELECT COUNT(*) FROM books")?;
        let count: u32 = stmt.query_row([], |row| row.get(0))?;
        Ok(count)
    }

    pub fn export_to_csv(&self, out_path: &Path) -> Result<()> {
        let mut stmt = self.conn.prepare(
            "SELECT nhen_id, title_1, title_2, artists, groups, tags, languages, pages
             FROM books ORDER BY id DESC",
        )?;

        let books = stmt.query_map([], |row| {
            Ok(BookData {
                id: row.get(0)?,
                title_1: row.get(1)?,
                title_2: row.get(2)?,
                artists: row.get(3)?,
                groups: row.get(4)?,
                tags: row.get(5)?,
                languages: row.get(6)?,
                pages: row.get(7)?,
            })
        })?;

        let mut wtr = csv::Writer::from_path(out_path)?;
        wtr.write_record(&[
            "nhen_id",
            "title_1",
            "title_2",
            "artists",
            "groups",
            "tags",
            "languages",
            "pages",
        ])?;

        for book_result in books {
            let book = book_result?;
            wtr.write_record(&[
                book.id,
                book.title_1,
                book.title_2,
                book.artists,
                book.groups,
                book.tags,
                book.languages,
                book.pages.to_string(),
            ])?;
        }

        wtr.flush()?;
        println!("[+] Exported database to {}", out_path.display());
        Ok(())
    }
}

pub fn backup_database(db_path: &Path, backup_dir: &Path) -> Result<()> {
    if !db_path.exists() {
        return Ok(());
    }

    std::fs::create_dir_all(backup_dir)?;
    let backup_path = backup_dir.join(db_path.file_name().unwrap());
    std::fs::copy(db_path, &backup_path)?;
    println!("[+] Database backed up to {}", backup_path.display());
    Ok(())
}

pub fn restore_database(db_path: &Path, backup_dir: &Path) -> Result<()> {
    let backup_path = backup_dir.join(db_path.file_name().unwrap());

    if !backup_path.exists() {
        println!(
            "[-] No previous database found at {}",
            backup_path.display()
        );
        return Ok(());
    }

    let db = Database::new(&backup_path)?;
    let prev_count = db.count_books()?;
    println!("[*] Previous database contains {} books", prev_count);

    if db_path.exists() {
        let current_db = Database::new(db_path)?;
        let current_count = current_db.count_books()?;
        println!("[*] Current database contains {} books", current_count);
        println!("[!] WARNING: Restoring will replace the current database");
    } else {
        println!("[*] No current database exists");
    }

    print!("\n[?] Do you want to restore from the previous database? (y/N): ");
    std::io::Write::flush(&mut std::io::stdout())?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    if input.trim().to_lowercase() != "y" {
        println!("[*] Restore cancelled");
        return Ok(());
    }

    std::fs::copy(&backup_path, db_path)?;
    println!(
        "[+] Database restored successfully from {}",
        backup_path.display()
    );
    Ok(())
}
