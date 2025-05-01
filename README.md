# Scoop GUI

一个为Windows上的Scoop包管理器提供图形界面的工具。

## 功能特点

- 直观的图形界面，使Scoop更易于使用
- 软件包搜索、安装、卸载和更新
- 桶(Bucket)管理
- 系统状态监控

## 技术栈

- Rust语言
- egui框架用于GUI
- Tokio用于异步操作

## 安装要求

- Windows操作系统
- 已安装Scoop包管理器
- Rust开发环境（如需编译）

## 使用方法

### 从源码编译

```bash
# 克隆仓库
git clone https://github.com/yourusername/scoopgui.git
cd scoopgui

# 编译并运行
cargo run

# 或者编译发布版本
cargo build --release
```

编译后的可执行文件将位于`target/release/scoopgui.exe`。

## 界面说明

### 首页

显示统计信息和快速操作按钮。

### 已安装软件

列出所有已安装的软件包，可以进行卸载或更新操作。

### 可用软件

搜索并显示可安装的软件包。

### 桶管理

管理Scoop的桶，包括添加和移除操作。

### 更新

检查和应用软件包更新。

### 设置

配置Scoop和GUI的选项。

## 开发计划

- [ ] 完善软件包安装/卸载/更新功能
- [ ] 添加软件包详情页面
- [ ] 实现桶添加/移除功能
- [ ] 添加主题支持
- [ ] 多语言支持

## 贡献

欢迎提交问题和拉取请求！

## 许可证

MIT
