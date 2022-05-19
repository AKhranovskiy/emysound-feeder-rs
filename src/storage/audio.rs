#![allow(dead_code)]

use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::Path;

use anyhow::anyhow;
use bytes::Bytes;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};
use rusqlite::{params, Connection, DatabaseName, OpenFlags, ToSql};
use uuid::Uuid;

pub struct AudioData {
    id: Uuid,
    format: AudioFormat,
    bytes: Bytes,
}

impl AudioData {
    pub fn new(id: Uuid, format: AudioFormat, bytes: Bytes) -> Self {
        Self { id, format, bytes }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AudioFormat {
    Aac,
}

impl ToSql for AudioFormat {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        match self {
            AudioFormat::Aac => "aac".to_sql(),
        }
    }
}

impl FromSql for AudioFormat {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        value.as_str().and_then(|v| match v {
            "aac" => Ok(AudioFormat::Aac),
            _ => Err(FromSqlError::InvalidType),
        })
    }
}

pub struct AudioStorage {
    conn: RefCell<Connection>,
}

impl AudioStorage {
    pub fn new<P>(path: &P) -> anyhow::Result<Self>
    where
        P: AsRef<Path>,
    {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS audio(
                id STRING PRIMARY KEY,
                format STRING NOT NULL,
                bytes BLOB NOT NULL
            )"#,
        )?;

        Ok(Self {
            conn: RefCell::new(conn),
        })
    }

    fn insert(&self, data: &AudioData) -> anyhow::Result<()> {
        let mut conn: std::cell::RefMut<Connection> = self.conn.borrow_mut();
        conn.transaction().and_then(|tx| {
            tx.execute(
                &format!(
                    "INSERT INTO audio VALUES(?, ?, ZEROBLOB({}))",
                    data.bytes.len()
                ),
                params![data.id.to_string(), data.format],
            )?;

            tx.blob_open(
                DatabaseName::Main,
                "audio",
                "bytes",
                tx.last_insert_rowid(),
                false,
            )?
            .write_all(data.bytes.as_ref())
            .map_err(|_| rusqlite::Error::BlobSizeError)?;

            tx.commit()
        })?;

        Ok(())
    }

    fn get(&self, id: Uuid) -> anyhow::Result<AudioData> {
        let conn = self.conn.borrow();
        let mut stmt = conn.prepare("SELECT rowid, id,format FROM audio WHERE id=?")?;
        let mut rows = stmt.query([id.to_string()])?;
        match rows.next() {
            Ok(Some(row)) => {
                let rowid = row.get(0)?;
                let id = Uuid::try_parse(&row.get::<usize, String>(1)?)?;
                let format: AudioFormat = row.get(2)?;

                let mut blob = conn.blob_open(DatabaseName::Main, "audio", "bytes", rowid, true)?;
                let mut buffer = Vec::new();
                blob.read_to_end(&mut buffer)?;
                Ok(AudioData::new(id, format, buffer.into()))
            }
            Ok(None) => Err(anyhow!("No results.")),
            Err(e) => Err(anyhow!("Query failed: {e:#}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{AudioData, AudioFormat, AudioStorage};

    #[test]
    fn test() {
        let data = AudioData::new(
            Uuid::new_v4(),
            AudioFormat::Aac,
            b"1234567890".as_ref().into(),
        );
        let db = AudioStorage::new(&"./test_audio.db").unwrap();
        db.insert(&data).unwrap();

        let result = db.get(data.id).unwrap();
        assert_eq!(result.id, data.id);
        assert_eq!(result.format, data.format);
        assert_eq!(result.bytes, data.bytes);
    }
}