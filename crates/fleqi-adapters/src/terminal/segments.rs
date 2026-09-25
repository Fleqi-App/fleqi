//! 输出分段落盘（architecture.md §6.3）：有上限的持久记录；超过上限记录明确截断边界。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const SEGMENT_BYTES: u64 = 8 * 1024 * 1024;

pub struct SegmentStore {
    dir: PathBuf,
    max_total: u64,
    written_total: u64,
    current: Option<(File, u64)>,
    index: u32,
    truncated: bool,
}

impl SegmentStore {
    pub fn open(dir: &Path, max_total: u64) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            max_total,
            written_total: 0,
            current: None,
            index: 0,
            truncated: false,
        })
    }

    pub fn append(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        if self.truncated {
            return Ok(());
        }
        if self.written_total + bytes.len() as u64 > self.max_total {
            self.truncated = true;
            let mark = format!(
                "\n[fleqi] 记录已截断：持久输出达到 {} 字节上限\n",
                self.max_total
            );
            let _ = self.write_raw(mark.as_bytes());
            return Ok(());
        }
        self.write_raw(bytes)
    }

    fn write_raw(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        if self
            .current
            .as_ref()
            .map(|(_, len)| *len >= SEGMENT_BYTES)
            .unwrap_or(true)
        {
            self.index += 1;
            let path = self.dir.join(format!("seg-{:04}.log", self.index));
            let file = OpenOptions::new().create(true).append(true).open(path)?;
            self.current = Some((file, 0));
        }
        if let Some((file, len)) = &mut self.current {
            file.write_all(bytes)?;
            *len += bytes.len() as u64;
            self.written_total += bytes.len() as u64;
        }
        Ok(())
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn written_total(&self) -> u64 {
        self.written_total
    }
}
