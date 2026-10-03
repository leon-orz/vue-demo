//! Toogl 共享核心 crate。
//! 桌面端 Tauri commands 与 hub HTTP 接口都是本 crate 的薄壳；
//! TS 类型由 ts-rs 从 models 生成（见 models::usage 的 export 测试）。

pub mod adapters;
pub mod datasource;
pub mod db;
pub mod keystore;
pub mod kimicode;
pub mod mihomo;
pub mod mimocode;
pub mod models;
pub mod omp;
pub mod sysproxy;
pub mod usage;
