//! 文件游标日志的公共扫描骨架：文件遍历、截断检测（size/mtime 变化即从头重扫）、
//! 逐文件容错、完整行读取循环。
//!
//! 注意：不是真正的"增量续读"——游标（offset + mtime）只用于变化检测，size/mtime
//! 任一变化即从头全量重扫，幂等性由 store 层整批重建（先删该文件旧事件再写入）保证。
//! 游标持久化由 store 层负责，本模块只做"文件系统 + 游标输入 → 事件 + 新游标"的纯转换。
//!
//! 平台差异由调用方注入：文件收集规则（`collect`）与单文件读取（`read`，内部经
//! [`read_lines`] 逐行回调做行解析与会话上下文提取）。

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::models::UsageEvent;

/// 单个文件的扫描结果
#[derive(Debug)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub size: u64,
    /// Unix 毫秒 mtime（取不到时为 0）
    pub mtime: i64,
    /// 本次读到的新偏移
    pub new_offset: u64,
    /// 触发从头重扫（截断/重建/mtime 变化）；store 层据此整批删除该文件旧事件
    pub truncated: bool,
    pub events: Vec<UsageEvent>,
}

/// 文件扫描游标（store 层持久化于 log_file 表，按 platform 隔离）
#[derive(Debug, Clone, Copy)]
pub struct FileCursor {
    pub offset: u64,
    /// Unix 毫秒
    pub mtime: i64,
}

#[derive(Debug, Default)]
pub struct ScanStats {
    pub files_found: u64,
    /// 因 size/mtime 变化而从头重扫的文件数（含普通追加触发的重扫）
    pub files_rescanned: u64,
    /// 取元数据 / 读取失败的文件数（跳过该文件且不更新游标，下次自然重试）
    pub files_failed: u64,
    pub events_parsed: u64,
    /// 含不完整尾行的文件数（写入半途的尾行不消费，下次重扫时覆盖）
    pub partial_lines_skipped: u64,
    pub bad_lines: u64,
}

#[derive(Debug)]
pub struct ScanOutput {
    pub files: Vec<ScannedFile>,
    pub stats: ScanStats,
}

/// 从头读取单个文件，对每个以换行符结尾的完整行回调 `on_line`。
/// 写入半途的不完整尾行不消费（计入 stats.partial_lines_skipped，留给下次重扫覆盖）。
/// 恒定从头读：骨架不做 offset 续读，任何变化都由 scan() 触发全量重扫。
/// 返回（已消费字节数, 回调产出的事件）。
pub fn read_lines<F>(
    path: &Path,
    stats: &mut ScanStats,
    mut on_line: F,
) -> std::io::Result<(u64, Vec<UsageEvent>)>
where
    F: FnMut(&str, &mut ScanStats, &mut Vec<UsageEvent>),
{
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut events = Vec::new();
    let mut consumed = 0u64;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break; // EOF
        }
        if !line.ends_with('\n') {
            // 不完整尾行：不前进偏移，留给下次重扫
            stats.partial_lines_skipped += 1;
            break;
        }
        consumed += n as u64;
        on_line(&line, stats, &mut events);
    }
    Ok((consumed, events))
}

/// 扫描 sessions 根目录。`cursors` 为 path → 游标（已读偏移 + mtime）的表（来自 store 层）。
/// `collect` 为平台的文件收集规则（递归匹配哪些文件）；`read` 为单文件读取
/// （恒定从头读，内部做行解析与会话上下文提取）。
/// 单文件容错：任一文件取元数据或读取失败只计入 `stats.files_failed` 并跳过
/// （不产出 ScannedFile，游标不更新，下次扫描自然重试），不中断整体扫描。
pub fn scan<C, R>(
    root: &Path,
    cursors: &HashMap<PathBuf, FileCursor>,
    collect: C,
    mut read: R,
) -> std::io::Result<ScanOutput>
where
    C: Fn(&Path, &mut Vec<PathBuf>) -> std::io::Result<()>,
    R: FnMut(&Path, &mut ScanStats) -> std::io::Result<(u64, Vec<UsageEvent>)>,
{
    let mut paths = Vec::new();
    collect(root, &mut paths)?;
    paths.sort();

    let mut stats = ScanStats::default();
    let mut files = Vec::new();
    for path in paths {
        stats.files_found += 1;
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                stats.files_failed += 1;
                continue;
            }
        };
        let size = meta.len();
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let cursor = cursors.get(&path).copied();
        let old_offset = cursor.map(|c| c.offset).unwrap_or(0);
        let old_mtime = cursor.map(|c| c.mtime).unwrap_or(0);
        // 截断检测放宽：大小或 mtime 任一变化都从头重扫（重扫幂等，代价低，
        // 可覆盖原地重写、内容替换但长度不变等 size < old_offset 抓不到的场景）
        let truncated =
            cursor.is_some() && (size != old_offset || (mtime != 0 && mtime != old_mtime));
        if truncated {
            stats.files_rescanned += 1;
        }
        if cursor.is_some() && !truncated {
            // 大小与 mtime 均未变化，无增量可读：跳过项目解析与读取，只更新 last_scanned_at
            files.push(ScannedFile {
                path,
                size,
                mtime,
                new_offset: old_offset,
                truncated: false,
                events: Vec::new(),
            });
            continue;
        }
        match read(&path, &mut stats) {
            Ok((new_offset, events)) => files.push(ScannedFile {
                path,
                size,
                mtime,
                new_offset,
                truncated,
                events,
            }),
            Err(_) => stats.files_failed += 1,
        }
    }
    Ok(ScanOutput { files, stats })
}
