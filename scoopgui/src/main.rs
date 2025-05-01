mod scoop;

use eframe::{egui, App, CreationContext};
use egui::{CentralPanel, SidePanel, TopBottomPanel, Ui, Vec2, Context};
use log::error;
use std::sync::{Arc, mpsc::{channel, Sender, Receiver}};
use tokio::runtime::Runtime;

use crate::scoop::{ScoopCommand, Package, Bucket};

// 应用状态结构体
// 定义后台线程发送给UI线程的消息类型
// 定义后台线程发送给UI线程的消息类型
#[derive(Debug)] // 添加Debug派生，方便调试
pub enum DataUpdate {
    Status(String),
    Loading(bool),
    InstalledPackages(Vec<Package>),
    Buckets(Vec<Bucket>),
    SearchResults(Vec<Package>),
    CommandOutput(String), // 新增：用于发送命令行输出
}

struct ScoopGui {
    // 当前选中的页面
    selected_tab: Tab,
    // 搜索框内容
    search_query: String,
    // 状态信息
    status_message: String,
    // 已安装的软件包
    installed_packages: Vec<Package>,
    // 搜索结果
    search_results: Vec<Package>,
    // 桶列表
    buckets: Vec<Bucket>,
    // 异步运行时
    runtime: Arc<Runtime>,
    // 是否正在加载数据
    is_loading: bool,
    // 命令行输出历史
    command_output: String,
    // 用于从后台线程接收数据更新
    data_receiver: Receiver<DataUpdate>,
    // 用于发送数据更新到UI线程 (克隆给后台线程)
    data_sender: Sender<DataUpdate>,
}

// 页面枚举
#[derive(PartialEq)]
enum Tab {
    Home,
    Installed,
    Available,
    Buckets,
    Updates,
    Settings,
}

impl Default for ScoopGui {
    fn default() -> Self {
        let runtime = Arc::new(Runtime::new().expect("Failed to create Tokio runtime"));
        let (data_sender, data_receiver) = channel();
        
        Self {
            selected_tab: Tab::Home,
            search_query: String::new(),
            status_message: String::from("就绪"),
            installed_packages: Vec::new(),
            search_results: Vec::new(),
            buckets: Vec::new(),
            runtime,
            is_loading: false,
            command_output: String::new(),
            data_receiver,
            data_sender,
        }
    }
}

impl App for ScoopGui {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // 处理来自后台线程的消息
        while let Ok(update) = self.data_receiver.try_recv() {
            match update {
                DataUpdate::Status(msg) => self.status_message = msg,
                DataUpdate::Loading(loading) => self.is_loading = loading,
                DataUpdate::InstalledPackages(packages) => self.installed_packages = packages,
                DataUpdate::Buckets(buckets) => self.buckets = buckets,
                DataUpdate::SearchResults(results) => self.search_results = results,
                DataUpdate::CommandOutput(output) => {
                    self.command_output.push_str(&output);
                    self.command_output.push('\n'); // 添加换行
                }
            }
            // 请求UI刷新以显示新接收的数据
            ctx.request_repaint();
        }

        // 顶部工具栏
        self.render_top_panel(ctx);
        
        // 侧边栏
        self.render_side_panel(ctx);
        
        // 主内容区
        self.render_main_panel(ctx);
        
        // 状态栏
        self.render_bottom_panel(ctx);
        
        // 如果正在加载，显示加载指示器
        if self.is_loading {
            self.render_loading_indicator(ctx);
        }
    }
}

impl ScoopGui {
    fn load_initial_data(&self) {
        let runtime = Arc::clone(&self.runtime);
        let sender = self.data_sender.clone();
        sender.send(DataUpdate::Loading(true)).ok();
        sender.send(DataUpdate::Status("正在加载初始数据...".to_string())).ok();

        std::thread::spawn(move || {
            runtime.block_on(async {
                let sender_clone1 = sender.clone();
                // 加载已安装包
                match ScoopCommand::list_installed(sender_clone1).await {
                    Ok(packages) => {
                        sender.send(DataUpdate::InstalledPackages(packages)).ok();
                    }
                    Err(e) => {
                        sender.send(DataUpdate::Status(format!("加载已安装包失败: {}", e))).ok();
                    }
                }

                let sender_clone2 = sender.clone();
                // 加载存储桶
                match ScoopCommand::list_buckets(sender_clone2).await {
                    Ok(buckets) => {
                        sender.send(DataUpdate::Buckets(buckets)).ok();
                    }
                    Err(e) => {
                        sender.send(DataUpdate::Status(format!("加载存储桶失败: {}", e))).ok();
                    }
                }

                sender.send(DataUpdate::Loading(false)).ok();
                sender.send(DataUpdate::Status("初始命令执行完成".to_string())).ok(); // 更新状态消息
            });
        });
    }
    
    fn perform_search(&mut self) {
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            self.status_message = "请输入搜索关键词".to_string();
            return;
        }

        self.is_loading = true;
        self.status_message = format!("正在搜索 '{}'...", query);
        self.search_results.clear(); // 清空旧结果

        let runtime = Arc::clone(&self.runtime);
        let sender = self.data_sender.clone();

        std::thread::spawn(move || {
            runtime.block_on(async {
                let sender_clone = sender.clone();
                // 执行搜索命令
                match ScoopCommand::search(&query, sender_clone).await {
                    Ok(results) => {
                        sender.send(DataUpdate::SearchResults(results)).ok();
                        sender.send(DataUpdate::Status("搜索完成".to_string())).ok();
                    }
                    Err(e) => {
                        sender.send(DataUpdate::Status(format!("搜索失败: {}", e))).ok();
                    }
                }
                sender.send(DataUpdate::Loading(false)).ok();
            });
        });
    }
    
    // 显示加载指示器
    fn render_loading_indicator(&self, ctx: &egui::Context) {
        egui::Window::new("加载中")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("正在加载数据，请稍候...");
            });
    }
    // 渲染顶部工具栏
    fn render_top_panel(&mut self, ctx: &egui::Context) {
        TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Scoop GUI");
                ui.separator();
                ui.label("搜索: ");
                let search_response = ui.text_edit_singleline(&mut self.search_query);
                let search_button = ui.button("🔍");
                if (search_response.lost_focus() && ui.ctx().input(|i| i.key_pressed(egui::Key::Enter))) || search_button.clicked() {
                    // 执行搜索
                    self.perform_search();
                    self.selected_tab = Tab::Available; // 切换到可用软件页面显示搜索结果
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙").clicked() {
                        self.selected_tab = Tab::Settings;
                    }
                    if ui.button("🔄").clicked() {
                        self.load_initial_data();
                    }
                });
            });
        });
    }

    // 渲染侧边栏
    fn render_side_panel(&mut self, ctx: &egui::Context) {
        SidePanel::left("side_panel").min_width(150.0).show(ctx, |ui| {
            ui.heading("导航");
            ui.separator();
            
            if ui.selectable_label(self.selected_tab == Tab::Home, "首页").clicked() {
                self.selected_tab = Tab::Home;
            }
            if ui.selectable_label(self.selected_tab == Tab::Installed, "已安装软件").clicked() {
                self.selected_tab = Tab::Installed;
            }
            if ui.selectable_label(self.selected_tab == Tab::Available, "可用软件").clicked() {
                self.selected_tab = Tab::Available;
            }
            if ui.selectable_label(self.selected_tab == Tab::Buckets, "桶管理").clicked() {
                self.selected_tab = Tab::Buckets;
            }
            if ui.selectable_label(self.selected_tab == Tab::Updates, "更新").clicked() {
                self.selected_tab = Tab::Updates;
            }
            if ui.selectable_label(self.selected_tab == Tab::Settings, "设置").clicked() {
                self.selected_tab = Tab::Settings;
            }
        });
    }

    // 渲染主内容区
    fn render_main_panel(&mut self, ctx: &egui::Context) {
        CentralPanel::default().show(ctx, |ui| {
            match self.selected_tab {
                Tab::Home => self.render_home_tab(ui),
                Tab::Installed => self.render_installed_tab(ui),
                Tab::Available => self.render_available_tab(ui),
                Tab::Buckets => self.render_buckets_tab(ui),
                Tab::Updates => self.render_updates_tab(ui),
                Tab::Settings => self.render_settings_tab(ui),
            }
        });
    }

    // 渲染底部面板（状态栏 + 命令行输出）
    fn render_bottom_panel(&mut self, ctx: &egui::Context) {
        // 使用可调整大小的底部面板
        TopBottomPanel::bottom("bottom_panel")
            .resizable(true)
            .min_height(30.0) // 最小高度，至少能显示状态栏
            .default_height(150.0) // 默认高度
            .show(ctx, |ui| {
                // 底部面板分为两部分：状态栏和命令行输出
                ui.vertical(|ui| {
                    // 1. 状态栏 (保持在最底部)
                    ui.horizontal(|ui| {
                        ui.label(self.status_message.clone());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label("Scoop GUI v0.1.0");
                        });
                    });
                    ui.separator();
                    
                    // 2. 命令行输出区域 (滚动)
                    ui.heading("命令行输出");
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true) // 自动滚动到底部
                        .auto_shrink([false, false]) // 填充可用空间
                        .show(ui, |ui| {
                            // 使用等宽字体显示输出
                            ui.add(egui::TextEdit::multiline(&mut self.command_output)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .interactive(false)); // 设置为只读
                        });
                });
            });
    }

    // 首页内容
    fn render_home_tab(&mut self, ui: &mut Ui) {
        ui.heading("欢迎使用 Scoop GUI");
        ui.label("Scoop 包管理器的图形界面");
        ui.separator();
        
        ui.heading("统计信息");
        ui.label(format!("已安装软件: {}", self.installed_packages.len()));
        ui.label("可用更新: 0"); // 将来实现
        ui.label(format!("已添加桶: {}", self.buckets.len()));
        
        ui.separator();
        ui.heading("快速操作");
        if ui.button("刷新软件列表").clicked() {
            self.load_initial_data();
        }
        if ui.button("检查更新").clicked() {
            self.status_message = String::from("检查更新...");
            // 将来实现
        }
    }

    // 已安装软件页面
    fn render_installed_tab(&mut self, ui: &mut Ui) {
        ui.heading("已安装软件");
        
        if self.installed_packages.is_empty() {
            ui.label("没有找到已安装的软件包");
            if ui.button("刷新").clicked() {
                self.load_initial_data();
            }
            return;
        }
        
        // 表格标题
        ui.horizontal(|ui| {
            ui.label("名称").on_hover_text("软件包名称");
            ui.add_space(150.0); // 列宽
            ui.label("版本").on_hover_text("当前安装的版本");
            ui.add_space(100.0);
            ui.label("操作").on_hover_text("可执行的操作");
        });
        
        ui.separator();
        
        // 表格内容
        egui::ScrollArea::vertical().show(ui, |ui| {
            for package in &self.installed_packages {
                ui.horizontal(|ui| {
                    ui.label(&package.name);
                    ui.add_space(150.0 - package.name.len() as f32 * 7.0); // 动态调整空间
                    ui.label(&package.version);
                    ui.add_space(100.0 - package.version.len() as f32 * 7.0);
                    if ui.button("卸载").clicked() {
                        // 将来实现卸载功能
                        self.status_message = format!("准备卸载: {}", package.name);
                    }
                    if ui.button("更新").clicked() {
                        // 将来实现更新功能
                        self.status_message = format!("准备更新: {}", package.name);
                    }
                });
                ui.separator();
            }
        });
    }

    // 可用软件页面
    fn render_available_tab(&mut self, ui: &mut Ui) {
        ui.heading("可用软件");
        
        // 搜索框
        ui.horizontal(|ui| {
            ui.label("搜索: ");
            let search_response = ui.text_edit_singleline(&mut self.search_query);
            let search_button = ui.button("搜索");
            if (search_response.lost_focus() && ui.ctx().input(|i| i.key_pressed(egui::Key::Enter))) || search_button.clicked() {
                self.perform_search();
            }
        });
        
        ui.separator();
        
        if self.search_results.is_empty() {
            ui.label("请输入搜索关键词查找软件包");
            return;
        }
        
        // 表格标题
        ui.horizontal(|ui| {
            ui.label("名称").on_hover_text("软件包名称");
            ui.add_space(150.0);
            ui.label("版本").on_hover_text("最新版本");
            ui.add_space(100.0);
            ui.label("操作").on_hover_text("可执行的操作");
        });
        
        ui.separator();
        
        // 表格内容
        egui::ScrollArea::vertical().show(ui, |ui| {
            for package in &self.search_results {
                ui.horizontal(|ui| {
                    ui.label(&package.name);
                    ui.add_space(150.0 - package.name.len() as f32 * 7.0);
                    ui.label(&package.version);
                    ui.add_space(100.0 - package.version.len() as f32 * 7.0);
                    if ui.button("安装").clicked() {
                        // 将来实现安装功能
                        self.status_message = format!("准备安装: {}", package.name);
                    }
                });
                ui.separator();
            }
        });
    }

    // 桶管理页面
    fn render_buckets_tab(&mut self, ui: &mut Ui) {
        ui.heading("桶管理");
        
        if self.buckets.is_empty() {
            ui.label("没有找到已添加的桶");
            if ui.button("刷新").clicked() {
                self.load_initial_data();
            }
            return;
        }
        
        ui.heading("已添加桶");
        
        // 表格标题
        ui.horizontal(|ui| {
            ui.label("名称").on_hover_text("桶名称");
            ui.add_space(150.0);
            ui.label("操作").on_hover_text("可执行的操作");
        });
        
        ui.separator();
        
        // 表格内容
        for bucket in &self.buckets {
            ui.horizontal(|ui| {
                ui.label(&bucket.name);
                ui.add_space(150.0 - bucket.name.len() as f32 * 7.0);
                if ui.button("移除").clicked() {
                    // 将来实现移除桶功能
                    self.status_message = format!("准备移除桶: {}", bucket.name);
                }
            });
            ui.separator();
        }
        
        ui.separator();
        ui.heading("添加新桶");
        
        // 推荐桶列表
        let recommended_buckets = ["extras", "versions", "nerd-fonts", "java", "games"];
        
        for bucket in recommended_buckets {
            let already_added = self.buckets.iter().any(|b| b.name == bucket);
            
            ui.horizontal(|ui| {
                ui.label(bucket);
                ui.add_space(150.0 - bucket.len() as f32 * 7.0);
                if already_added {
                    ui.label("已添加");
                } else if ui.button("添加").clicked() {
                    // 将来实现添加桶功能
                    self.status_message = format!("准备添加桶: {}", bucket);
                }
            });
            ui.separator();
        }
    }

    // 更新页面
    fn render_updates_tab(&mut self, ui: &mut Ui) {
        ui.heading("软件更新");
        ui.label("此页面将显示可更新的软件列表");
        // 这里将来会显示可更新软件的列表
    }

    // 设置页面
    fn render_settings_tab(&mut self, ui: &mut Ui) {
        ui.heading("设置");
        ui.label("此页面将提供Scoop和GUI的设置选项");
        // 这里将来会显示设置选项
    }
}

use egui::FontFamily::Proportional;
use egui::{FontData, FontDefinitions, TextStyle};

impl ScoopGui {
    fn new(cc: &CreationContext<'_>) -> Self {
        // 配置字体以支持中文
        let mut fonts = FontDefinitions::default();

        // 添加中文字体 (例如：微软雅黑)
        // 你可以根据需要替换为其他字体，如 "SimHei" 或嵌入字体文件
        fonts.font_data.insert(
            "my_chinese_font".to_owned(),
            FontData::from_static(include_bytes!("C:/Windows/Fonts/msyh.ttc")), // 尝试加载微软雅黑
                                                                            // 如果msyh.ttc不存在或路径错误，需要调整
                                                                            // 或者使用其他字体如 Deng.ttf (等线)
        );

        // 将中文字体添加到 Proportional 和 Monospace 字体族
        // 确保它在默认字体之后，但在备用字体之前
        if let Some(family) = fonts.families.get_mut(&Proportional) {
            family.insert(1, "my_chinese_font".to_owned()); // 插入到第二优先级
        }
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            family.insert(1, "my_chinese_font".to_owned());
        }

        // 应用字体配置
        cc.egui_ctx.set_fonts(fonts);

        let app = Self::default();
        // 应用启动时加载数据
        app.load_initial_data();
        app
    }
}

fn main() {
    env_logger::init(); // 初始化日志
    
    let native_options = eframe::NativeOptions {
        initial_window_size: Some(Vec2::new(1024.0, 768.0)),
        ..Default::default()
    };
    
    eframe::run_native(
        "Scoop GUI",
        native_options,
        Box::new(|cc| Box::new(ScoopGui::new(cc)))
    );
}
