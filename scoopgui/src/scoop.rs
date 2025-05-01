use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::sync::mpsc::Sender;
use tokio::process::Command as TokioCommand;
use tokio::io::{AsyncBufReadExt, BufReader};

// 导入main.rs中定义的DataUpdate枚举
// 注意：这需要在main.rs中将DataUpdate设为pub
// 或者将DataUpdate移动到单独的模块中
// 这里假设它在main.rs中且为pub
use crate::DataUpdate;

/// Scoop软件包信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub installed: bool,
    pub bucket: Option<String>,
}

/// Scoop桶信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    pub name: String,
    pub source: String,
    pub updated: Option<String>,
}

/// Scoop命令执行器
pub struct ScoopCommand;

impl ScoopCommand {
    /// 执行Scoop命令，并将实时输出通过Sender发送，成功后返回stdout
    pub async fn execute(args: &[&str], sender: Sender<DataUpdate>) -> Result<String> {
        let command_str = format!("scoop {}", args.join(" "));
        sender.send(DataUpdate::CommandOutput(format!("> {}", command_str))).ok();

        let mut child = TokioCommand::new("powershell")
            .arg("-Command")
            .arg(&command_str)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let stderr = child.stderr.take().expect("Failed to capture stderr");

        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        let stdout_sender = sender.clone();
        let stderr_sender = sender.clone();

        // 用于收集stdout
        let mut stdout_collector = String::new();

        let stdout_handle = tokio::spawn(async move {
            while let Ok(Some(line)) = stdout_reader.next_line().await {
                stdout_sender.send(DataUpdate::CommandOutput(line.clone())).ok();
                stdout_collector.push_str(&line);
                stdout_collector.push('\n');
            }
            stdout_collector // 返回收集到的stdout
        });

        let stderr_handle = tokio::spawn(async move {
            while let Ok(Some(line)) = stderr_reader.next_line().await {
                stderr_sender.send(DataUpdate::CommandOutput(format!("ERR: {}", line))).ok();
            }
        });

        // 等待命令完成和输出读取完成
        let status = child.wait().await?;
        // 等待 stdout 任务完成并获取收集到的字符串
        let collected_stdout = match stdout_handle.await {
            Ok(s) => Ok(s), // 成功获取到 stdout 字符串, 返回 Ok(String)
            Err(join_err) => {
                // 发送 JoinError 信息
                sender.send(DataUpdate::CommandOutput(format!("ERR: stdout task join failed: {}", join_err))).ok();
                Err(anyhow!("stdout task join failed: {}", join_err)) // 返回 Err
            }
        }?;

        // 等待 stderr 任务完成
        match stderr_handle.await {
            Ok(_) => Ok(()), // stderr 任务正常完成, 返回 Ok(())
            Err(join_err) => {
                 // 发送 JoinError 信息
                sender.send(DataUpdate::CommandOutput(format!("ERR: stderr task join failed: {}", join_err))).ok();
                Err(anyhow!("stderr task join failed: {}", join_err)) // 返回 Err
            }
        }?;

        if status.success() {
            sender.send(DataUpdate::CommandOutput("命令执行成功".to_string())).ok();
            Ok(collected_stdout) // 返回 Ok 包装的 String
        } else {
            let err_msg = format!("命令执行失败，退出码: {:?}", status.code());
            sender.send(DataUpdate::CommandOutput(err_msg.clone())).ok();
            Err(anyhow!(err_msg))
        }
    }

    /// 检查Scoop是否已安装
    pub async fn is_installed() -> bool {
        let output = TokioCommand::new("powershell")
            .arg("-Command")
            .arg("Get-Command scoop -ErrorAction SilentlyContinue")
            .output()
            .await;

        match output {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// 获取已安装的软件包列表
    pub async fn list_installed(sender: Sender<DataUpdate>) -> Result<Vec<Package>> {
        let output = Self::execute(&["list"], sender).await?;
        
        let mut packages = Vec::new();
        let mut header_skipped = false;
        let mut separator_skipped = false;

        for line in output.lines() {
            let trimmed_line = line.trim();
            if trimmed_line.is_empty() {
                continue;
            }

            // 跳过标题行 "Name Version Source Updated"
            if !header_skipped {
                if trimmed_line.starts_with("Name") {
                    header_skipped = true;
                }
                continue;
            }

            // 跳过分隔行 "---- ------- ------ -------"
            if !separator_skipped {
                if trimmed_line.starts_with("----") {
                    separator_skipped = true;
                }
                continue;
            }

            // 解析包信息行
            let parts: Vec<&str> = trimmed_line.split_whitespace().collect();
            if parts.len() >= 2 { // 至少需要名字和版本
                packages.push(Package {
                    name: parts[0].to_string(),
                    version: parts[1].to_string(),
                    description: None, // 'scoop list' 不提供描述
                    installed: true,   // 'scoop list' 只列出已安装的
                    bucket: if parts.len() > 2 { Some(parts[2].to_string()) } else { None }, // 有些可能没有来源桶
                });
            }
        }

        Ok(packages)
    }

    /// 搜索软件包
    pub async fn search(query: &str, sender: Sender<DataUpdate>) -> Result<Vec<Package>> {
        let output = Self::execute(&["search", query], sender).await?;
        
        let mut packages = Vec::new();
        let mut current_bucket = String::new();
        let mut header_skipped = false;
        let mut separator_skipped = false;

        for line in output.lines() {
            let trimmed_line = line.trim();
            if trimmed_line.is_empty() {
                // 重置状态以处理下一个桶
                header_skipped = false;
                separator_skipped = false;
                continue;
            }

            // 检查是否是桶名行
            if !trimmed_line.contains(' ') && trimmed_line.ends_with(':') {
                current_bucket = trimmed_line.trim_end_matches(':').to_string();
                header_skipped = false; // 每个桶都有自己的头和分隔符
                separator_skipped = false;
                continue;
            }

            // 跳过标题行 "Name Version Info"
            if !header_skipped {
                if trimmed_line.starts_with("Name") {
                    header_skipped = true;
                }
                continue;
            }

            // 跳过分隔行 "---- ------- ----"
            if !separator_skipped {
                if trimmed_line.starts_with("----") {
                    separator_skipped = true;
                }
                continue;
            }

            // 解析包信息行
            let parts: Vec<&str> = trimmed_line.split_whitespace().collect();
            if parts.len() >= 2 { // 至少需要名字和版本
                packages.push(Package {
                    name: parts[0].to_string(),
                    version: parts[1].to_string(),
                    description: if parts.len() > 2 { Some(parts[2..].join(" ")) } else { None }, // Info 部分可能包含空格
                    installed: false, // 'scoop search' 不直接表明是否安装，需要额外检查
                    bucket: Some(current_bucket.clone()),
                });
            }
        }

        Ok(packages)
    }

    /// 获取可用的桶列表
    pub async fn list_buckets(sender: Sender<DataUpdate>) -> Result<Vec<Bucket>> {
        let output = Self::execute(&["bucket", "list"], sender).await?;
        
        let mut buckets = Vec::new();
        let mut header_skipped = false;
        let mut separator_skipped = false;

        for line in output.lines() {
            let trimmed_line = line.trim();
            if trimmed_line.is_empty() {
                continue;
            }

            // 跳过标题行 "Name Source Updated Manifests"
            if !header_skipped {
                if trimmed_line.starts_with("Name") {
                    header_skipped = true;
                }
                continue;
            }

            // 跳过分隔行 "---- ------ ------- ---------"
            if !separator_skipped {
                if trimmed_line.starts_with("----") {
                    separator_skipped = true;
                }
                continue;
            }

            // 解析桶信息行
            let parts: Vec<&str> = trimmed_line.split_whitespace().collect();
            if parts.len() >= 2 { // 至少需要名字和来源
                buckets.push(Bucket {
                    name: parts[0].to_string(),
                    source: parts[1].to_string(),
                    updated: if parts.len() > 2 { Some(parts[2..parts.len()-1].join(" ")) } else { None }, // 更新时间可能包含空格，最后一个是Manifests数量
                });
            }
        }

        Ok(buckets)
    }

    /// 安装软件包
    pub async fn install(package_name: &str, sender: Sender<DataUpdate>) -> Result<String> {
        Self::execute(&["install", package_name], sender).await
    }

    /// 卸载软件包
    pub async fn uninstall(package_name: &str, sender: Sender<DataUpdate>) -> Result<String> {
        Self::execute(&["uninstall", package_name], sender).await
    }

    /// 更新软件包
    pub async fn update(package_name: &str, sender: Sender<DataUpdate>) -> Result<String> {
        Self::execute(&["update", package_name], sender).await
    }

    /// 添加桶
    pub async fn add_bucket(bucket_name: &str, sender: Sender<DataUpdate>) -> Result<String> {
        Self::execute(&["bucket", "add", bucket_name], sender).await
    }

    /// 移除桶
    pub async fn remove_bucket(bucket_name: &str, sender: Sender<DataUpdate>) -> Result<String> {
        Self::execute(&["bucket", "rm", bucket_name], sender).await
    }
}