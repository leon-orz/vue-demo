//! Windows 系统工具的实际实现。
//!
//! 桌面端命令层只负责 IPC 编排；系统信息、临时目录扫描和网络修复逻辑
//! 保持在核心 crate，便于复用并集中维护安全边界。

use std::ffi::OsStr;
use std::net::{SocketAddr, TcpStream};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::PathBuf;
use std::time::Duration;

use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

use crate::models::{
    CleanupScan, CommandError, NetworkDiagnostic, NetworkRepairResult, SystemInformation,
};

const MAX_SCAN_ENTRIES: usize = 50_000;
const DNS_PROBE: &str = "example.com:443";

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

pub fn system_information() -> Result<SystemInformation, CommandError> {
    let mut memory = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    if unsafe { GlobalMemoryStatusEx(&mut memory) } == 0 {
        return Err(CommandError::unavailable("无法读取系统内存信息"));
    }

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let current_version = hklm
        .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
        .map_err(|e| CommandError::unavailable(format!("无法读取 Windows 版本: {e}")))?;
    let processor_key = hklm
        .open_subkey("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0")
        .map_err(|e| CommandError::unavailable(format!("无法读取处理器信息: {e}")))?;
    let product: String = current_version.get_value("ProductName").unwrap_or_default();
    let display_version: String = current_version
        .get_value("DisplayVersion")
        .unwrap_or_default();
    let build: String = current_version
        .get_value("CurrentBuildNumber")
        .unwrap_or_default();
    let processor: String = processor_key
        .get_value("ProcessorNameString")
        .unwrap_or_default();
    let root = std::env::var_os("SystemDrive").unwrap_or_else(|| "C:".into());
    let root = PathBuf::from(format!("{}\\", root.to_string_lossy()));
    let mut free = 0u64;
    let mut total = 0u64;
    if unsafe {
        GetDiskFreeSpaceExW(
            wide(root.as_os_str()).as_ptr(),
            &mut free,
            &mut total,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(CommandError::unavailable("无法读取系统盘空间信息"));
    }

    let build_number = build.parse::<u32>().unwrap_or_default();
    let edition = if build_number >= 22_000 {
        product.replace("Windows 10", "Windows 11")
    } else {
        product
    };
    Ok(SystemInformation {
        windows_edition: edition,
        version: format!("{display_version} (内部版本 {build})"),
        architecture: std::env::var("PROCESSOR_ARCHITECTURE").unwrap_or_else(|_| "未知".into()),
        computer_name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "未知".into()),
        processor,
        total_memory_bytes: memory.ullTotalPhys,
        system_drive_free_bytes: free,
        system_drive_total_bytes: total,
    })
}

pub fn scan_temp_directory() -> Result<CleanupScan, CommandError> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| CommandError::unavailable("无法确定当前用户本地应用数据目录"))?;
    scan_temp_directory_at(&PathBuf::from(local_app_data).join("Temp"))
}

fn scan_temp_directory_at(temp: &std::path::Path) -> Result<CleanupScan, CommandError> {
    let metadata = std::fs::symlink_metadata(temp)
        .map_err(|e| CommandError::unavailable(format!("无法访问当前用户临时目录: {e}")))?;
    if metadata.file_attributes()
        & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
    {
        return Err(CommandError::unavailable(
            "当前用户临时目录是重解析点，已拒绝扫描",
        ));
    }
    let root = std::fs::canonicalize(temp)
        .map_err(|e| CommandError::unavailable(format!("无法访问当前用户临时目录: {e}")))?;
    let mut pending = vec![root.clone()];
    let mut result = CleanupScan {
        files: 0,
        bytes: 0,
        skipped_entries: 0,
    };
    let mut visited = 0usize;

    while let Some(directory) = pending.pop() {
        let entries = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(_) => {
                result.skipped_entries += 1;
                continue;
            }
        };
        for entry in entries {
            visited += 1;
            if visited > MAX_SCAN_ENTRIES {
                result.skipped_entries += 1;
                return Ok(result);
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    result.skipped_entries += 1;
                    continue;
                }
            };
            let metadata = match std::fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(_) => {
                    result.skipped_entries += 1;
                    continue;
                }
            };
            let attributes = metadata.file_attributes();
            if attributes & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
                != 0
            {
                result.skipped_entries += 1;
            } else if metadata.is_dir() && entry.path().starts_with(&root) {
                pending.push(entry.path());
            } else if metadata.is_file() {
                result.files += 1;
                result.bytes = result.bytes.saturating_add(metadata.len());
            }
        }
    }
    Ok(result)
}

fn network_diagnostic(dns_resolution: bool, tcp_connection: bool) -> NetworkDiagnostic {
    let message = match (dns_resolution, tcp_connection) {
        (false, _) => "域名解析失败或超时，请检查 DNS 配置或网络连接".into(),
        (true, false) => "DNS 正常，但无法连接测试站点；可能存在防火墙或网络限制".into(),
        (true, true) => "DNS 解析与外网 TCP 连接正常".into(),
    };
    NetworkDiagnostic {
        dns_resolution,
        tcp_connection,
        message,
    }
}

pub async fn diagnose_network() -> NetworkDiagnostic {
    let addresses =
        tokio::time::timeout(Duration::from_secs(5), tokio::net::lookup_host(DNS_PROBE)).await;
    let address: Option<SocketAddr> = match addresses {
        Ok(Ok(addresses)) => addresses.into_iter().next(),
        _ => None,
    };
    let Some(address) = address else {
        return network_diagnostic(false, false);
    };
    let tcp_connection = tokio::task::spawn_blocking(move || {
        TcpStream::connect_timeout(&address, Duration::from_secs(3)).is_ok()
    })
    .await
    .unwrap_or(false);
    network_diagnostic(true, tcp_connection)
}

pub fn flush_dns() -> Result<NetworkRepairResult, CommandError> {
    let system_root = std::env::var_os("SystemRoot")
        .ok_or_else(|| CommandError::unavailable("无法确定 Windows 系统目录"))?;
    let executable = PathBuf::from(system_root).join("System32\\ipconfig.exe");
    let output = std::process::Command::new(executable)
        .arg("/flushdns")
        .output()
        .map_err(|e| CommandError::unavailable(format!("无法启动 DNS 缓存刷新: {e}")))?;
    if output.status.success() {
        Ok(NetworkRepairResult {
            success: true,
            message: "Windows DNS 客户端缓存已刷新".into(),
        })
    } else {
        Err(CommandError::internal(format!(
            "DNS 缓存刷新失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}
#[cfg(test)]
mod tests {
    use super::scan_temp_directory_at;
    use std::fs;

    #[test]
    fn scan_counts_files_without_modifying_them() {
        let temp = tempfile::tempdir().unwrap();
        let nested = temp.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let file = nested.join("cache.bin");
        fs::write(&file, b"cache-data").unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("outside.bin");
        fs::write(&outside_file, b"outside").unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(outside.path(), temp.path().join("linked-outside"))
            .unwrap();

        let result = scan_temp_directory_at(temp.path()).unwrap();

        assert_eq!(result.files, 1);
        assert_eq!(result.bytes, 10);
        #[cfg(windows)]
        assert_eq!(result.skipped_entries, 1);
        assert_eq!(fs::read(file).unwrap(), b"cache-data");
        assert_eq!(fs::read(outside_file).unwrap(), b"outside");
    }

    #[cfg(windows)]
    #[test]
    fn scan_rejects_reparse_point_as_root() {
        use std::os::windows::fs::symlink_dir;

        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = temp.path().join("linked-temp");
        symlink_dir(target, &link).unwrap();

        assert!(scan_temp_directory_at(&link).is_err());
    }
}
