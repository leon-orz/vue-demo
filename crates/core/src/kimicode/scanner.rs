//! 增量扫描器：遍历 sessions 目录下的 wire.jsonl，按游标续读，产出归一化事件。
//! 游标持久化由 store 层负责，本模块只做"文件系统 + 游标输入 → 事件 + 新游标"的纯转换。

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::parser::{self, ParsedUsage};
use crate::models::KimiUsageEvent;

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
    pub events: Vec<KimiUsageEvent>,
}

/// 文件扫描游标（store 层持久化于 kimi_log_file 表）
#[derive(Debug, Clone, Copy)]
pub struct FileCursor {
    pub offset: u64,
    /// Unix 毫秒
    pub mtime: i64,
}

#[derive(Debug, Default)]
pub struct ScanStats {
    pub files_found: u64,
    pub files_rescanned: u64,
    /// 取元数据 / 读取失败的文件数（跳过该文件且不更新游标，下次自然重试）
    pub files_failed: u64,
    pub events_parsed: u64,
    /// 含不完整尾行的文件数（写入半途的尾行不消费，下次续读覆盖）
    pub partial_lines_skipped: u64,
    pub bad_lines: u64,
}

#[derive(Debug)]
pub struct ScanOutput {
    pub files: Vec<ScannedFile>,
    pub stats: ScanStats,
}

/// 会话元信息（state.json 子集）：cwd 提供项目名，id 提供会话标识
#[derive(Debug, Deserialize)]
struct SessionState {
    id: Option<String>,
    cwd: Option<String>,
}

/// 递归收集目录下所有 wire.jsonl（结构为 sessions/{wd_*}/{session_*}/agents/*/wire.jsonl，
/// 但按文件名递归匹配，不依赖固定层级）
fn collect_wire_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_wire_files(&path, out)?;
        } else if entry.file_name() == "wire.jsonl" {
            out.push(path);
        }
    }
    Ok(())
}

/// 从 wire.jsonl 推导会话目录（…/{session_*}/agents/{agent}/wire.jsonl → {session_*}）
fn session_dir(wire_path: &Path) -> Option<&Path> {
    wire_path.parent()?.parent()?.parent()
}

/// 项目名：state.json 的 cwd 末级目录名；取不到回退 "unknown"
fn project_of(wire_path: &Path) -> (String, String) {
    let fallback = || ("unknown".to_string(), "unknown".to_string());
    let Some(session_dir) = session_dir(wire_path) else {
        return fallback();
    };
    let session_fallback = || {
        session_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".to_string())
    };
    let Ok(content) = std::fs::read_to_string(session_dir.join("state.json")) else {
        return (fallback().0, session_fallback());
    };
    let Ok(state) = serde_json::from_str::<SessionState>(&content) else {
        return (fallback().0, session_fallback());
    };
    let session_id = state.id.unwrap_or_else(session_fallback);
    let project = state
        .cwd
        .as_deref()
        .and_then(|cwd| {
            // cwd 可能是 "C:/zone/2demo" 或 "D:\\proj\\app"，统一按两种分隔符取末级
            let trimmed = cwd.trim_end_matches(['/', '\\']);
            trimmed.rsplit(['/', '\\']).next().filter(|s| !s.is_empty())
        })
        .unwrap_or("unknown")
        .to_string();
    (project, session_id)
}

/// 从 offset 续读单个文件。只处理以换行符结尾的完整行（写入半途的尾行不消费，下次续读覆盖）。
fn read_incremental(
    path: &Path,
    offset: u64,
    project: &str,
    session_id: &str,
    stats: &mut ScanStats,
) -> std::io::Result<(u64, Vec<KimiUsageEvent>)> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::new(file);
    let mut events = Vec::new();
    let mut consumed = offset;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break; // EOF
        }
        if !line.ends_with('\n') {
            // 不完整尾行：不前进偏移，留给下次续读
            stats.partial_lines_skipped += 1;
            break;
        }
        consumed += n as u64;
        match parser::parse_line(&line) {
            Ok(Some(ParsedUsage {
                ts,
                model,
                input_other,
                output,
                cache_read,
                cache_creation,
            })) => {
                events.push(KimiUsageEvent {
                    ts,
                    session_id: session_id.to_string(),
                    project: project.to_string(),
                    model,
                    input_other,
                    output,
                    cache_read,
                    cache_creation,
                });
                stats.events_parsed += 1;
            }
            Ok(None) => {}
            Err(_) => stats.bad_lines += 1,
        }
    }
    Ok((consumed, events))
}

/// 扫描 sessions 根目录。`cursors` 为 path → 游标（已读偏移 + mtime）的表（来自 store 层）。
/// 单文件容错：任一文件取元数据或读取失败只计入 `stats.files_failed` 并跳过
/// （不产出 ScannedFile，游标不更新，下次扫描自然重试），不中断整体扫描。
pub fn scan(root: &Path, cursors: &HashMap<PathBuf, FileCursor>) -> std::io::Result<ScanOutput> {
    let mut paths = Vec::new();
    collect_wire_files(root, &mut paths)?;
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
        let (project, session_id) = project_of(&path);
        match read_incremental(&path, 0, &project, &session_id, &mut stats) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 在临时目录构造一个会话：state.json + agents/main/wire.jsonl
    fn make_session(root: &Path, wd: &str, session: &str, cwd: &str, lines: &[&str]) -> PathBuf {
        let session_dir = root.join(wd).join(session);
        let agents = session_dir.join("agents").join("main");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(
            session_dir.join("state.json"),
            format!(r#"{{"id":"{session}","cwd":{cwd:?}}}"#),
        )
        .unwrap();
        let wire = agents.join("wire.jsonl");
        let mut f = File::create(&wire).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        wire
    }

    fn usage_line(ts: i64, input: i64, output: i64) -> String {
        format!(
            r#"{{"type":"usage.record","model":"kimi-code/k1","usage":{{"inputOther":{input},"output":{output},"inputCacheRead":0,"inputCacheCreation":0}},"usageScope":"turn","time":{ts}}}"#
        )
    }

    #[test]
    fn scans_multiple_sessions_and_projects() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        make_session(
            root,
            "wd_app_aaa",
            "session_s1",
            "C:/work/app",
            &[&usage_line(1000, 10, 5), r#"{"type":"session.start"}"#],
        );
        make_session(
            root,
            "wd_blog_bbb",
            "session_s2",
            "D:\\proj\\blog",
            &[&usage_line(2000, 20, 7)],
        );

        let out = scan(root, &HashMap::new()).unwrap();
        assert_eq!(out.stats.files_found, 2);
        assert_eq!(out.stats.events_parsed, 2);
        assert_eq!(out.stats.bad_lines, 0);

        let mut events: Vec<_> = out.files.iter().flat_map(|f| f.events.clone()).collect();
        events.sort_by_key(|e| e.ts);
        assert_eq!(events[0].project, "app");
        assert_eq!(events[0].session_id, "session_s1");
        assert_eq!(events[0].input_other, 10);
        assert_eq!(events[1].project, "blog");
        assert_eq!(events[1].session_id, "session_s2");
    }

    #[test]
    fn bad_lines_counted_without_interrupting() {
        let tmp = tempfile::tempdir().unwrap();
        let wire = make_session(
            tmp.path(),
            "wd_app_aaa",
            "session_s1",
            "C:/work/app",
            &[
                "{broken",
                &usage_line(1000, 1, 1),
                "",
                &usage_line(2000, 2, 2),
            ],
        );
        let out = scan(tmp.path(), &HashMap::new()).unwrap();
        assert_eq!(out.stats.bad_lines, 1);
        assert_eq!(out.stats.events_parsed, 2);
        assert_eq!(out.files[0].path, wire);
    }

    #[test]
    fn appended_file_fully_rescanned() {
        let tmp = tempfile::tempdir().unwrap();
        let wire = make_session(
            tmp.path(),
            "wd_app_aaa",
            "session_s1",
            "C:/work/app",
            &[&usage_line(1000, 10, 5)],
        );
        let first = scan(tmp.path(), &HashMap::new()).unwrap();
        let cursor = FileCursor {
            offset: first.files[0].new_offset,
            mtime: first.files[0].mtime,
        };
        assert_eq!(first.stats.events_parsed, 1);

        // 追加两行后再扫：size 变化触发从头重扫（读到全部 3 行，幂等由 store 层保证）
        let mut f = File::options().append(true).open(&wire).unwrap();
        writeln!(f, "{}", usage_line(2000, 20, 6)).unwrap();
        writeln!(f, "{}", usage_line(3000, 30, 7)).unwrap();
        drop(f);

        let cursors = HashMap::from([(wire.clone(), cursor)]);
        let second = scan(tmp.path(), &cursors).unwrap();
        assert!(second.files[0].truncated);
        assert_eq!(second.stats.events_parsed, 3);
        assert!(second.files[0].new_offset > cursor.offset);
    }

    #[test]
    fn unchanged_file_skipped_without_reading() {
        let tmp = tempfile::tempdir().unwrap();
        let wire = make_session(
            tmp.path(),
            "wd_app_aaa",
            "session_s1",
            "C:/work/app",
            &[&usage_line(1000, 10, 5)],
        );
        let first = scan(tmp.path(), &HashMap::new()).unwrap();
        let cursor = FileCursor {
            offset: first.files[0].new_offset,
            mtime: first.files[0].mtime,
        };

        // size 与 mtime 均未变化：跳过读取，只回传游标（store 层仅更新 last_scanned_at）
        let cursors = HashMap::from([(wire, cursor)]);
        let second = scan(tmp.path(), &cursors).unwrap();
        assert_eq!(second.stats.files_found, 1);
        assert_eq!(second.stats.events_parsed, 0);
        assert_eq!(second.stats.files_rescanned, 0);
        assert!(!second.files[0].truncated);
        assert!(second.files[0].events.is_empty());
        assert_eq!(second.files[0].new_offset, cursor.offset);
    }

    #[test]
    fn same_size_rewrite_detected_via_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let wire = make_session(
            tmp.path(),
            "wd_app_aaa",
            "session_s1",
            "C:/work/app",
            &[&usage_line(1000, 10, 5)],
        );
        let first = scan(tmp.path(), &HashMap::new()).unwrap();
        let cursor = FileCursor {
            offset: first.files[0].new_offset,
            mtime: first.files[0].mtime,
        };

        // 原地重写为等长内容（长度不变，只有 mtime 变化）：也应触发重扫
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&wire, format!("{}\n", usage_line(9999, 99, 9))).unwrap();

        let cursors = HashMap::from([(wire, cursor)]);
        let second = scan(tmp.path(), &cursors).unwrap();
        assert!(second.files[0].truncated);
        assert_eq!(second.stats.files_rescanned, 1);
        assert_eq!(second.files[0].events.len(), 1);
        assert_eq!(second.files[0].events[0].ts, 9999);
    }

    #[test]
    fn truncated_file_rescanned_from_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let wire = make_session(
            tmp.path(),
            "wd_app_aaa",
            "session_s1",
            "C:/work/app",
            &[&usage_line(1000, 10, 5), &usage_line(2000, 20, 6)],
        );
        let first = scan(tmp.path(), &HashMap::new()).unwrap();
        let cursor = FileCursor {
            offset: first.files[0].new_offset,
            mtime: first.files[0].mtime,
        };

        // 重建为更短的文件
        std::fs::write(&wire, format!("{}\n", usage_line(500, 1, 1))).unwrap();

        let cursors = HashMap::from([(wire, cursor)]);
        let second = scan(tmp.path(), &cursors).unwrap();
        assert!(second.files[0].truncated);
        assert_eq!(second.stats.files_rescanned, 1);
        assert_eq!(second.stats.events_parsed, 1);
        assert_eq!(second.files[0].events[0].ts, 500);
    }

    #[test]
    fn partial_tail_line_not_consumed() {
        let tmp = tempfile::tempdir().unwrap();
        let wire = make_session(
            tmp.path(),
            "wd_app_aaa",
            "session_s1",
            "C:/work/app",
            &[&usage_line(1000, 10, 5)],
        );
        // 追加一行写到一半（无换行结尾）
        let mut f = File::options().append(true).open(&wire).unwrap();
        write!(f, "{}", usage_line(2000, 20, 6)).unwrap();
        drop(f);

        let first = scan(tmp.path(), &HashMap::new()).unwrap();
        assert_eq!(first.stats.events_parsed, 1);
        assert_eq!(first.stats.partial_lines_skipped, 1);
        let cursor = FileCursor {
            offset: first.files[0].new_offset,
            mtime: first.files[0].mtime,
        };

        // 补全该行后再扫：size 变化触发重扫，两条事件都能读到（不完整尾行不会被重复消费）
        let mut f = File::options().append(true).open(&wire).unwrap();
        writeln!(f).unwrap();
        drop(f);
        let cursors = HashMap::from([(wire, cursor)]);
        let second = scan(tmp.path(), &cursors).unwrap();
        assert_eq!(second.stats.events_parsed, 2);
        assert_eq!(second.stats.partial_lines_skipped, 0);
    }

    /// 单文件容错：被独占占用（不可读）的文件计入 files_failed 并跳过，不影响其他文件。
    /// Windows 上用 share_mode(0) 独占打开模拟；本项目仅支持 Windows（NF-14）。
    #[cfg(windows)]
    #[test]
    fn unreadable_file_counted_and_skipped() {
        use std::os::windows::fs::OpenOptionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let good = make_session(
            tmp.path(),
            "wd_good",
            "session_g",
            "C:/work/good",
            &[&usage_line(1000, 1, 1)],
        );
        let bad = make_session(
            tmp.path(),
            "wd_bad",
            "session_b",
            "C:/work/bad",
            &[&usage_line(2000, 2, 2)],
        );
        // 不共享任何访问，File::open 将失败
        let _lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&bad)
            .unwrap();

        let out = scan(tmp.path(), &HashMap::new()).unwrap();
        assert_eq!(out.stats.files_found, 2);
        assert_eq!(out.stats.files_failed, 1);
        // 只有好的文件产出结果；失败文件不出现（游标不更新，下次自然重试）
        assert_eq!(out.files.len(), 1);
        assert_eq!(out.files[0].path, good);
        assert_eq!(out.stats.events_parsed, 1);
    }

    #[test]
    fn missing_state_json_falls_back_to_unknown() {
        let tmp = tempfile::tempdir().unwrap();
        let agents = tmp
            .path()
            .join("wd_x")
            .join("session_s1")
            .join("agents")
            .join("main");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(
            agents.join("wire.jsonl"),
            format!("{}\n", usage_line(1000, 3, 1)),
        )
        .unwrap();

        let out = scan(tmp.path(), &HashMap::new()).unwrap();
        assert_eq!(out.stats.events_parsed, 1);
        assert_eq!(out.files[0].events[0].project, "unknown");
        assert_eq!(out.files[0].events[0].session_id, "session_s1");
    }
}
