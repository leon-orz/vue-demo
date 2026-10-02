//! 增量扫描器：遍历 sessions 目录下的会话 jsonl，按游标续读，产出归一化事件。
//! 游标持久化由 store 层负责，本模块只做"文件系统 + 游标输入 → 事件 + 新游标"的纯转换。
//!
//! 复用 kimicode::scanner 的游标/结果类型与截断检测模式（size 或 mtime 变化即从头重扫，
//! 重扫幂等由 store 层整批重建保证）；差异在文件收集（*.jsonl 按扩展名递归匹配，排除
//! 会话子目录里的 .bash.log）与会话元信息来源（文件内 type=="session" 行，而非旁路
//! state.json——session 行实测固定出现在消息行之前，从头重扫时天然先读到）。

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub use crate::kimicode::scanner::{FileCursor, ScanOutput, ScanStats, ScannedFile};

use super::parser::{self, ParsedUsage};
use super::OMP_CLI;
use crate::models::{Platform, UsageEvent};

/// 递归收集目录下所有 *.jsonl（结构为 sessions/{--cwd--}/{ts}_{sessionId}.jsonl，
/// 按扩展名匹配，不依赖固定层级；会话子目录中的 .bash.log 等自然排除）
fn collect_session_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_session_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "jsonl") {
            out.push(path);
        }
    }
    Ok(())
}

/// 会话标识兜底：文件名形如 `{开始时间}_{sessionId}.jsonl`，取最后一段 `_` 之后；
/// 取不到回退完整文件名（不带扩展名）
fn session_id_fallback(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string());
    stem.rsplit('_')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(&stem)
        .to_string()
}

/// 项目名：cwd 末级目录名；cwd 可能是 "C:/zone/2demo" 或 "C:\\zone\\2demo"，
/// 统一按两种分隔符取末级；取不到回退 "unknown"
fn project_from_cwd(cwd: &str) -> String {
    let trimmed = cwd.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

/// 从 offset 续读单个文件。只处理以换行符结尾的完整行（写入半途的尾行不消费，下次续读覆盖）。
/// 逐行维护会话元信息（session 行先于消息行出现），事件归属读到时的最新会话上下文。
fn read_incremental(
    path: &Path,
    offset: u64,
    stats: &mut ScanStats,
) -> std::io::Result<(u64, Vec<UsageEvent>)> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut reader = BufReader::new(file);
    let mut events = Vec::new();
    let mut consumed = offset;
    let mut line = String::new();
    // 会话上下文：session 行出现前用文件名/unknown 兜底
    let mut session_id: Option<String> = None;
    let mut project: Option<String> = None;
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
        if let Some(meta) = parser::parse_session_meta(&line) {
            if meta.id.is_some() {
                session_id = meta.id;
            }
            if let Some(cwd) = meta.cwd {
                project = Some(project_from_cwd(&cwd));
            }
            continue;
        }
        match parser::parse_line(&line) {
            Ok(Some(ParsedUsage {
                ts,
                model,
                input_other,
                output,
                cache_read,
                cache_creation,
                cost,
                error_stop,
            })) => {
                events.push(UsageEvent {
                    platform: Platform::Omp,
                    cli: OMP_CLI.to_string(),
                    ts,
                    session_id: session_id
                        .clone()
                        .unwrap_or_else(|| session_id_fallback(path)),
                    project: project.clone().unwrap_or_else(|| "unknown".to_string()),
                    model,
                    input_other,
                    output,
                    cache_read,
                    cache_creation,
                    cost,
                });
                stats.events_parsed += 1;
                if error_stop {
                    // 异常行（stopReason 异常 / 带 errorMessage）单列计数需求合并进 bad_lines
                    // （ScanReport 无独立字段；事件本身已正常入库，此处仅为诊断计数）
                    stats.bad_lines += 1;
                }
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
    collect_session_files(root, &mut paths)?;
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
            // 大小与 mtime 均未变化，无增量可读：跳过读取，只更新 last_scanned_at
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
        match read_incremental(&path, 0, &mut stats) {
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

    /// 在临时目录构造一个 omp 会话：{--cwd--}/{ts}_{sessionId}.jsonl
    fn make_session(root: &Path, dir: &str, session: &str, cwd: &str, lines: &[&str]) -> PathBuf {
        let session_dir = root.join(dir);
        std::fs::create_dir_all(&session_dir).unwrap();
        let file = session_dir.join(format!("2026-09-21T06-56-06-434Z_{session}.jsonl"));
        let mut f = File::create(&file).unwrap();
        // 真实文件第 1 行为 title、第 2 行为 session 元信息
        writeln!(f, r#"{{"type":"title","title":"t"}}"#).unwrap();
        writeln!(
            f,
            r#"{{"type":"session","version":3,"id":"{session}","timestamp":"2026-09-21T06:56:06.434Z","cwd":{cwd:?}}}"#
        )
        .unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        file
    }

    fn assistant_line(ts: i64, input: i64, output: i64) -> String {
        format!(
            r#"{{"type":"message","timestamp":"2026-09-21T07:18:03.566Z","message":{{"role":"assistant","provider":"deepseek","model":"deepseek-v4-flash","usage":{{"input":{input},"output":{output},"cacheRead":0,"cacheWrite":0,"cost":{{"total":0.001}}}},"stopReason":"stop","timestamp":{ts}}}}}"#
        )
    }

    #[test]
    fn scans_multiple_sessions_and_projects() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        make_session(
            root,
            "--C--work-app--",
            "s1",
            "C:/work/app",
            &[&assistant_line(1000, 10, 5), r#"{"type":"model_change"}"#],
        );
        make_session(
            root,
            "--D--proj-blog--",
            "s2",
            "D:\\proj\\blog",
            &[&assistant_line(2000, 20, 7)],
        );

        let out = scan(root, &HashMap::new()).unwrap();
        assert_eq!(out.stats.files_found, 2);
        assert_eq!(out.stats.events_parsed, 2);
        assert_eq!(out.stats.bad_lines, 0);

        let mut events: Vec<_> = out.files.iter().flat_map(|f| f.events.clone()).collect();
        events.sort_by_key(|e| e.ts);
        assert_eq!(events[0].platform, Platform::Omp);
        assert_eq!(events[0].cli, "omp");
        assert_eq!(events[0].project, "app");
        assert_eq!(events[0].session_id, "s1");
        assert_eq!(events[0].input_other, 10);
        assert_eq!(events[0].model, "deepseek/deepseek-v4-flash");
        assert_eq!(events[0].cost, Some(0.001));
        assert_eq!(events[1].project, "blog");
        assert_eq!(events[1].session_id, "s2");
    }

    #[test]
    fn bad_and_error_lines_counted_without_interrupting() {
        let tmp = tempfile::tempdir().unwrap();
        make_session(
            tmp.path(),
            "--C--work-app--",
            "s1",
            "C:/work/app",
            &[
                "{broken",
                &assistant_line(1000, 1, 1),
                "",
                // 异常行：stopReason=aborted（计入 bad_lines，事件仍入库）
                r#"{"type":"message","message":{"role":"assistant","model":"m","usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0},"stopReason":"aborted","errorMessage":"Interrupted by user","timestamp":1500}}"#,
                &assistant_line(2000, 2, 2),
            ],
        );
        let out = scan(tmp.path(), &HashMap::new()).unwrap();
        // bad_lines = 1 损坏行 + 1 异常行
        assert_eq!(out.stats.bad_lines, 2);
        assert_eq!(out.stats.events_parsed, 3);
    }

    #[test]
    fn appended_file_fully_rescanned() {
        let tmp = tempfile::tempdir().unwrap();
        let file = make_session(
            tmp.path(),
            "--C--work-app--",
            "s1",
            "C:/work/app",
            &[&assistant_line(1000, 10, 5)],
        );
        let first = scan(tmp.path(), &HashMap::new()).unwrap();
        let cursor = FileCursor {
            offset: first.files[0].new_offset,
            mtime: first.files[0].mtime,
        };
        assert_eq!(first.stats.events_parsed, 1);

        // 追加两行后再扫：size 变化触发从头重扫（读到全部 3 条事件，幂等由 store 层保证）
        let mut f = File::options().append(true).open(&file).unwrap();
        writeln!(f, "{}", assistant_line(2000, 20, 6)).unwrap();
        writeln!(f, "{}", assistant_line(3000, 30, 7)).unwrap();
        drop(f);

        let cursors = HashMap::from([(file.clone(), cursor)]);
        let second = scan(tmp.path(), &cursors).unwrap();
        assert!(second.files[0].truncated);
        assert_eq!(second.stats.events_parsed, 3);
        assert!(second.files[0].new_offset > cursor.offset);
    }

    #[test]
    fn unchanged_file_skipped_without_reading() {
        let tmp = tempfile::tempdir().unwrap();
        let file = make_session(
            tmp.path(),
            "--C--work-app--",
            "s1",
            "C:/work/app",
            &[&assistant_line(1000, 10, 5)],
        );
        let first = scan(tmp.path(), &HashMap::new()).unwrap();
        let cursor = FileCursor {
            offset: first.files[0].new_offset,
            mtime: first.files[0].mtime,
        };

        // size 与 mtime 均未变化：跳过读取，只回传游标（store 层仅更新 last_scanned_at）
        let cursors = HashMap::from([(file, cursor)]);
        let second = scan(tmp.path(), &cursors).unwrap();
        assert_eq!(second.stats.files_found, 1);
        assert_eq!(second.stats.events_parsed, 0);
        assert_eq!(second.stats.files_rescanned, 0);
        assert!(!second.files[0].truncated);
        assert!(second.files[0].events.is_empty());
        assert_eq!(second.files[0].new_offset, cursor.offset);
    }

    #[test]
    fn truncated_file_rescanned_from_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let file = make_session(
            tmp.path(),
            "--C--work-app--",
            "s1",
            "C:/work/app",
            &[&assistant_line(1000, 10, 5), &assistant_line(2000, 20, 6)],
        );
        let first = scan(tmp.path(), &HashMap::new()).unwrap();
        let cursor = FileCursor {
            offset: first.files[0].new_offset,
            mtime: first.files[0].mtime,
        };

        // 重建为更短的文件
        std::fs::write(&file, format!("{}\n", assistant_line(500, 1, 1))).unwrap();

        let cursors = HashMap::from([(file, cursor)]);
        let second = scan(tmp.path(), &cursors).unwrap();
        assert!(second.files[0].truncated);
        assert_eq!(second.stats.files_rescanned, 1);
        assert_eq!(second.stats.events_parsed, 1);
        assert_eq!(second.files[0].events[0].ts, 500);
        // 重建后无 session 行：回退文件名推导会话标识、unknown 项目
        assert_eq!(second.files[0].events[0].session_id, "s1");
        assert_eq!(second.files[0].events[0].project, "unknown");
    }

    #[test]
    fn partial_tail_line_not_consumed() {
        let tmp = tempfile::tempdir().unwrap();
        let file = make_session(
            tmp.path(),
            "--C--work-app--",
            "s1",
            "C:/work/app",
            &[&assistant_line(1000, 10, 5)],
        );
        // 追加一行写到一半（无换行结尾）
        let mut f = File::options().append(true).open(&file).unwrap();
        write!(f, "{}", assistant_line(2000, 20, 6)).unwrap();
        drop(f);

        let first = scan(tmp.path(), &HashMap::new()).unwrap();
        assert_eq!(first.stats.events_parsed, 1);
        assert_eq!(first.stats.partial_lines_skipped, 1);
        let cursor = FileCursor {
            offset: first.files[0].new_offset,
            mtime: first.files[0].mtime,
        };

        // 补全该行后再扫：size 变化触发重扫，两条事件都能读到（不完整尾行不会被重复消费）
        let mut f = File::options().append(true).open(&file).unwrap();
        writeln!(f).unwrap();
        drop(f);
        let cursors = HashMap::from([(file, cursor)]);
        let second = scan(tmp.path(), &cursors).unwrap();
        assert_eq!(second.stats.events_parsed, 2);
        assert_eq!(second.stats.partial_lines_skipped, 0);
    }

    #[test]
    fn non_jsonl_files_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let file = make_session(
            tmp.path(),
            "--C--work-app--",
            "s1",
            "C:/work/app",
            &[&assistant_line(1000, 1, 1)],
        );
        // 会话子目录里的 .bash.log 不应被收集
        let sub = tmp.path().join("--C--work-app--").join("2026-09-21_s1");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("0.bash.log"), "noise").unwrap();

        let out = scan(tmp.path(), &HashMap::new()).unwrap();
        assert_eq!(out.stats.files_found, 1);
        assert_eq!(out.files[0].path, file);
    }
}
