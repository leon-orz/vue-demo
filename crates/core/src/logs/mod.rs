//! 文件游标日志的公共扫描/入库骨架（kimicode wire.jsonl 与 omp 会话 jsonl 共用）。
//!
//! 两个平台的文件型日志扫描曾各有一份逐字重复的骨架（scan 主循环、截断检测、
//! 游标 upsert、截断整批重建、日聚合重算、扫描互斥锁），现统一收在本模块；
//! 平台差异收敛为调用方注入的三样东西：
//! - 文件收集规则（递归匹配哪些文件）
//! - 行解析回调（一行 → 零或一条 UsageEvent，可在回调内维护会话上下文）
//! - platform / cli 两个常量
//!
//! 边界：mimocode 不纳入本骨架——它的日志源是单个 SQLite 库（mimocode.db 只读），
//! 增量以 message 表 rowid 为行游标（不是文件 size/mtime 游标），读取循环与
//! 截断检测语义都不同，强行套用只会引入没有收益的参数化。

pub mod scanner;
pub mod store;
