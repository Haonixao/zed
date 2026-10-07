use chrono::Local;
use collections::HashMap;
use gpui::AppContext;
use gpui::WeakEntity;
use gpui::*;
use gpui::{App, AsyncApp, Context};
use serde::Deserialize;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use task::{
    HideStrategy, RevealStrategy, RevealTarget, SaveStrategy, Shell, SpawnInTerminal, TaskId,
};
use terminal_view::terminal_panel::TerminalPanel;
use ui::{Button, Color, Icon, IconButton, IconName, IconSize, Label, LabelSize, prelude::*};
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

#[derive(Debug, Clone)]
struct Container {
    id: String,
    names: String,
    image: String,
    state: String,
    ports: String,
    has_bash: bool,
    project_name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Image {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Repository")]
    repository: String,
    #[serde(rename = "Tag")]
    tag: String,
    #[serde(rename = "Size")]
    size: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Volume {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Driver")]
    driver: String,
}

pub struct DockerPanel {
    containers: Vec<Container>,
    images: Vec<Image>,
    volumes: Vec<Volume>,
    status: String,
    focus_handle: FocusHandle,
    workspace: WeakEntity<Workspace>,
    containers_expanded: bool,
    images_expanded: bool,
    volumes_expanded: bool,
    container_info_expanded: std::collections::HashMap<String, bool>,
    container_groups_expanded: std::collections::HashMap<String, bool>,
}

impl DockerPanel {
    pub fn new(cx: &mut Context<Self>, _window: &mut Window, workspace: Entity<Workspace>) -> Self {
        let mut this = Self {
            containers: vec![],
            images: vec![],
            volumes: vec![],
            status: "Not updated".to_string(),
            focus_handle: cx.focus_handle(),
            workspace: workspace.downgrade(),
            containers_expanded: true,
            images_expanded: false,
            volumes_expanded: false,
            container_info_expanded: std::collections::HashMap::new(),
            container_groups_expanded: std::collections::HashMap::new(),
        };
        this.refresh(cx);
        this
    }

    fn docker_cmd() -> std::process::Command {
        let cmd = std::process::Command::new("docker");

        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000);
        }

        cmd
    }

    fn render_container(&self, c: &Container, cx: &mut Context<Self>) -> impl IntoElement {
        let is_running = c.state == "running";
        let short_id = c.id.chars().take(12).collect::<String>();

        v_flex()
            .id(format!("container-{}", short_id))
            .p_2()
            .rounded_md()
            .bg(cx.theme().colors().panel_background)
            .border_1()
            .border_color(cx.theme().colors().border)
            .gap_2()
            // Имя + статус
            .child(
                h_flex().justify_between().items_center().child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Label::new(&c.names).weight(FontWeight::MEDIUM))
                        .child(
                            Label::new(if is_running { "R" } else { "S" })
                                .size(LabelSize::Small)
                                .color(if is_running {
                                    Color::Success
                                } else {
                                    Color::Error
                                })
                                .weight(FontWeight::SEMIBOLD),
                        ),
                ),
            )
            // Info-блок (раскрывающийся)
            .child(
                h_flex()
                    .id(format!("info-header-{}", short_id))
                    .justify_between()
                    .items_center()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .hover(|style| style.bg(cx.theme().colors().ghost_element_hover))
                    .on_click(cx.listener({
                        let id = c.id.clone();
                        move |this, _, _, cx| {
                            this.toggle_container_info(&id, cx);
                        }
                    }))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Icon::new(
                                    if self
                                        .container_info_expanded
                                        .get(&c.id)
                                        .copied()
                                        .unwrap_or(false)
                                    {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    },
                                )
                                .size(IconSize::Small),
                            )
                            .child(
                                Label::new("Info")
                                    .size(LabelSize::Small)
                                    .weight(FontWeight::MEDIUM),
                            ),
                    ),
            )
            .when(
                self.container_info_expanded
                    .get(&c.id)
                    .copied()
                    .unwrap_or(false),
                |this| {
                    this.child(
                        v_flex()
                            .gap_1()
                            .pl_6()
                            .child(
                                Label::new(format!("Image: {}", &c.image))
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(
                                Label::new(format!(
                                    "Ports: {}",
                                    if c.ports.is_empty() { "no" } else { &c.ports }
                                ))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                            ),
                    )
                },
            )
            // Кнопки управления
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        IconButton::new(format!("start-{}", short_id), IconName::PlayFilled)
                            .icon_size(IconSize::Small)
                            .disabled(is_running)
                            .on_click(cx.listener({
                                let id = c.id.clone();
                                move |this, _, _, cx| {
                                    this.docker_action("start", id.as_str(), cx);
                                }
                            })),
                    )
                    .child(
                        IconButton::new(format!("stop-{}", short_id), IconName::Stop)
                            .icon_size(IconSize::Small)
                            .disabled(!is_running)
                            .on_click(cx.listener({
                                let id = c.id.clone();
                                move |this, _, _, cx| {
                                    this.docker_action("stop", id.as_str(), cx);
                                }
                            })),
                    )
                    .child(
                        IconButton::new(format!("restart-{}", short_id), IconName::RotateCw)
                            .icon_size(IconSize::Small)
                            .on_click(cx.listener({
                                let id = c.id.clone();
                                move |this, _, _, cx| {
                                    this.docker_action("restart", id.as_str(), cx);
                                }
                            })),
                    )
                    .child(
                        IconButton::new(format!("logs-{}", short_id), IconName::Notepad)
                            .icon_size(IconSize::Small)
                            .on_click(cx.listener({
                                let id = c.id.clone();
                                move |this, _, window, cx| {
                                    this.show_logs(window, cx, &id);
                                }
                            })),
                    )
                    .child(
                        IconButton::new(format!("exec-{}", short_id), IconName::Terminal)
                            .icon_size(IconSize::Small)
                            .disabled(!is_running)
                            .on_click(cx.listener({
                                let id = c.id.clone();
                                let has_bash = c.has_bash;
                                move |this, _, window, cx| {
                                    this.exec_container(window, cx, &id, has_bash);
                                }
                            })),
                    )
                    .child(
                        IconButton::new(format!("remove-{}", short_id), IconName::Trash)
                            .icon_size(IconSize::Small)
                            .on_click(cx.listener({
                                let id = c.id.clone();
                                move |this, _, _, cx| {
                                    let _ = DockerPanel::docker_cmd()
                                        .arg("rm")
                                        .arg("-f")
                                        .arg(&id)
                                        .output();
                                    this.refresh(cx);
                                }
                            })),
                    ),
            )
    }

    fn toggle_images(&mut self, cx: &mut Context<Self>) {
        self.images_expanded = !self.images_expanded;
        cx.notify();
    }

    fn toggle_volumes(&mut self, cx: &mut Context<Self>) {
        self.volumes_expanded = !self.volumes_expanded;
        cx.notify();
    }

    fn remove_image(&mut self, image_id: &str, cx: &mut Context<Self>) {
        let _ = DockerPanel::docker_cmd()
            .arg("rmi")
            .arg("-f")
            .arg(image_id)
            .output();
        self.refresh(cx);
    }

    fn remove_volume(&mut self, volume_name: &str, cx: &mut Context<Self>) {
        let _ = DockerPanel::docker_cmd()
            .arg("volume")
            .arg("rm")
            .arg("-f")
            .arg(volume_name)
            .output();
        self.refresh(cx);
    }

    fn toggle_container_group(&mut self, project_name: &str, cx: &mut Context<Self>) {
        let current = self
            .container_groups_expanded
            .get(project_name)
            .copied()
            .unwrap_or(false);
        self.container_groups_expanded
            .insert(project_name.to_string(), !current);
        cx.notify();
    }

    fn toggle_containers(&mut self, cx: &mut Context<Self>) {
        self.containers_expanded = !self.containers_expanded;
        cx.notify();
    }

    fn toggle_container_info(&mut self, container_id: &str, cx: &mut Context<Self>) {
        let current = self
            .container_info_expanded
            .get(container_id)
            .copied()
            .unwrap_or(false);
        self.container_info_expanded
            .insert(container_id.to_string(), !current);
        cx.notify();
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.status = format!("Updating... {}", Local::now().format("%H:%M:%S"));

        // === Containers ===
        let ps_output = DockerPanel::docker_cmd()
            .arg("ps")
            .arg("-a")
            .arg("--format")
            .arg("{{.ID}}")
            .output();

        let container_ids: Vec<String> = match ps_output {
            Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.trim().to_string())
                .collect(),
            Ok(_) => {
                self.status = "docker ps error".to_string();
                return;
            }
            Err(e) => {
                self.status = format!("docker not found: {}", e);
                return;
            }
        };

        if container_ids.is_empty() {
            self.containers.clear();
            self.status = format!("Updated {}", Local::now().format("%H:%M:%S"));
            cx.notify();
            return;
        }

        let mut inspect_cmd = DockerPanel::docker_cmd();
        inspect_cmd
            .arg("inspect")
            .args(&container_ids)
            .arg("--format")
            .arg("{{json .}}");

        match inspect_cmd.output() {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);

                let containers_data: Vec<serde_json::Value> = text
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .filter_map(|line| serde_json::from_str(line).ok())
                    .collect();

                self.containers = containers_data
                    .iter()
                    .filter_map(|c| {
                        let id = c["Id"].as_str()?.to_string();

                        let names = c["Name"]
                            .as_str()
                            .unwrap_or("")
                            .trim_start_matches('/')
                            .to_string();

                        let image = c["Config"]["Image"]
                            .as_str()
                            .unwrap_or("unknown")
                            .to_string();

                        let state = c["State"]["Status"]
                            .as_str()
                            .unwrap_or("unknown")
                            .to_string();

                        let labels = c["Config"]["Labels"].as_object();

                        let project_name = labels
                            .and_then(|l| l.get("com.docker.compose.project"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("Other")
                            .to_string();

                        // Порты
                        let mut ports_str = String::new();
                        if let Some(bindings) = c["HostConfig"]["PortBindings"].as_object() {
                            for (container_port, host_bindings) in bindings {
                                if let Some(arr) = host_bindings.as_array() {
                                    for binding in arr {
                                        if let Some(h_port) = binding["HostPort"].as_str() {
                                            if !ports_str.is_empty() {
                                                ports_str.push_str(", ");
                                            }
                                            ports_str.push_str(&format!(
                                                "{}->{}",
                                                h_port, container_port
                                            ));
                                        }
                                    }
                                }
                            }
                        }

                        // Проверка bash только для running
                        let has_bash = if state == "running" {
                            DockerPanel::docker_cmd()
                                .arg("exec")
                                .arg(&id)
                                .arg("which")
                                .arg("bash")
                                .output()
                                .map(|o| o.status.success())
                                .unwrap_or(false)
                        } else {
                            false
                        };

                        Some(Container {
                            id,
                            names,
                            image,
                            state,
                            ports: ports_str,
                            has_bash,
                            project_name,
                        })
                    })
                    .collect();

                // Очистка состояний раскрытия
                self.container_info_expanded
                    .retain(|id, _| self.containers.iter().any(|c| &c.id == id));
                // Очистка состояний раскрытия групп: оставляем только те проекты,
                // которые реально присутствуют среди текущих контейнеров
                self.container_groups_expanded
                    .retain(|p, _| self.containers.iter().any(|c| &c.project_name == p));
            }
            Ok(_) => self.status = "docker inspect error".to_string(),
            Err(e) => self.status = format!("docker inspect failed: {}", e),
        }

        // === Images ===
        let images_output = DockerPanel::docker_cmd()
            .arg("images")
            .arg("--format")
            .arg("{{json .}}")
            .output();

        match images_output {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                self.images = text
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .filter_map(|line| serde_json::from_str::<Image>(line).ok())
                    .collect();
            }
            _ => {}
        }

        // === Volumes ===
        let volumes_output = DockerPanel::docker_cmd()
            .arg("volume")
            .arg("ls")
            .arg("--format")
            .arg("{{json .}}")
            .output();

        match volumes_output {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                self.volumes = text
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .filter_map(|line| serde_json::from_str::<Volume>(line).ok())
                    .collect();
            }
            _ => {}
        }

        self.status = format!("Updated {}", Local::now().format("%H:%M:%S"));
        cx.notify();
    }

    fn docker_action(&mut self, action: &str, container_id: &str, cx: &mut Context<Self>) {
        let _ = DockerPanel::docker_cmd()
            .arg(action)
            .arg(container_id)
            .output();
        self.refresh(cx);
    }

    fn docker_group_action(&mut self, action: &str, project_name: &str, cx: &mut Context<Self>) {
        for c in self.containers.iter() {
            if c.project_name == project_name {
                let _ = DockerPanel::docker_cmd().arg(action).arg(&c.id).output();
            }
        }
        self.refresh(cx);
    }

    fn show_logs(&self, window: &mut Window, cx: &mut Context<Self>, container_id: &str) {
        let short_id = container_id.chars().take(12).collect::<String>();
        eprintln!(
            " [Docker Logs] show_logs called for container: {}",
            short_id
        );

        let Some(workspace) = self.workspace.upgrade() else {
            eprintln!(" [Docker Logs] Failed to upgrade workspace");
            return;
        };

        let spawn_task = SpawnInTerminal {
            id: TaskId(format!("docker-logs-{}", short_id)),
            full_label: format!(" Logs — {}", short_id),
            label: format!(" Logs — {}", short_id),
            command_label: format!("docker logs -f {}", short_id),
            command: Some("docker".into()),
            args: vec!["logs".into(), "-f".into(), container_id.into()],
            cwd: None,
            env: HashMap::default(),
            use_new_terminal: true,
            allow_concurrent_runs: true,
            reveal: RevealStrategy::Always,
            reveal_target: RevealTarget::Dock,
            hide: HideStrategy::Never,
            shell: Shell::System,
            show_summary: true,
            show_command: true,
            show_rerun: true,
            save: SaveStrategy::None,
        };

        eprintln!(" [Docker Logs] SpawnInTerminal created");

        let task_handle = workspace.update(cx, |workspace, cx| {
            let _ = workspace.toggle_panel_focus::<TerminalPanel>(window, cx);

            if let Some(terminal_panel) = workspace.panel::<TerminalPanel>(cx) {
                terminal_panel.update(cx, |terminal_panel, cx| {
                    terminal_panel.add_terminal_task(spawn_task, RevealStrategy::Always, window, cx)
                })
            } else {
                Task::ready(Err(anyhow::anyhow!("TerminalPanel not found")))
            }
        });

        cx.spawn({
            let task_handle = task_handle;
            move |_this: WeakEntity<Self>, _cx: &mut AsyncApp| async move {
                match task_handle.await {
                    Ok(weak_terminal) => {
                        eprintln!(
                            " [Docker Logs] Terminal created successfully: {:?}",
                            weak_terminal
                        );
                    }
                    Err(e) => {
                        eprintln!(" [Docker Logs] Error creating terminal: {:?}", e);
                    }
                }
            }
        })
        .detach();

        eprintln!(" [Docker Logs] show_logs finished");
    }

    fn exec_container(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        container_id: &str,
        has_bash: bool,
    ) {
        let short_id = container_id.chars().take(12).collect::<String>();
        let shell = if has_bash { "/bin/bash" } else { "/bin/sh" };
        eprintln!(
            " [Docker Exec] exec_container called for container: {} (bash: {})",
            short_id, has_bash
        );

        let Some(workspace) = self.workspace.upgrade() else {
            eprintln!(" [Docker Exec] Failed to upgrade workspace");
            return;
        };

        let spawn_task = SpawnInTerminal {
            id: TaskId(format!("docker-exec-{}", short_id)),
            full_label: format!(" Exec — {}", short_id),
            label: format!(" Exec — {}", short_id),
            command_label: format!("docker exec -it {} {}", short_id, shell),
            command: Some("docker".into()),
            args: vec![
                "exec".into(),
                "-it".into(),
                container_id.into(),
                shell.into(),
            ],
            cwd: None,
            env: HashMap::default(),
            use_new_terminal: true,
            allow_concurrent_runs: true,
            reveal: RevealStrategy::Always,
            reveal_target: RevealTarget::Dock,
            hide: HideStrategy::Never,
            shell: Shell::System,
            show_summary: true,
            show_command: true,
            show_rerun: true,
            save: SaveStrategy::None,
        };

        eprintln!(" [Docker Exec] SpawnInTerminal created");

        let task_handle = workspace.update(cx, |workspace, cx| {
            let _ = workspace.toggle_panel_focus::<TerminalPanel>(window, cx);

            if let Some(terminal_panel) = workspace.panel::<TerminalPanel>(cx) {
                terminal_panel.update(cx, |terminal_panel, cx| {
                    terminal_panel.add_terminal_task(spawn_task, RevealStrategy::Always, window, cx)
                })
            } else {
                Task::ready(Err(anyhow::anyhow!("TerminalPanel not found")))
            }
        });

        cx.spawn({
            let task_handle = task_handle;
            move |_this: WeakEntity<Self>, _cx: &mut AsyncApp| async move {
                match task_handle.await {
                    Ok(weak_terminal) => {
                        eprintln!(
                            " [Docker Exec] Terminal created successfully: {:?}",
                            weak_terminal
                        );
                    }
                    Err(e) => {
                        eprintln!(" [Docker Exec] Error creating terminal: {:?}", e);
                    }
                }
            }
        })
        .detach();

        eprintln!(" [Docker Exec] exec_container finished");
    }
}

impl Render for DockerPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Группировка контейнеров по project_name
        let mut grouped: std::collections::HashMap<String, Vec<&Container>> =
            std::collections::HashMap::new();
        for c in &self.containers {
            grouped
                .entry(c.project_name.clone())
                .or_insert_with(Vec::new)
                .push(c);
        }

        // Сортировка: Other — последним, остальные по алфавиту
        let mut group_names: Vec<String> = grouped.keys().cloned().collect();
        group_names.sort_by(|a, b| {
            if a == "Other" {
                std::cmp::Ordering::Greater
            } else if b == "Other" {
                std::cmp::Ordering::Less
            } else {
                a.cmp(b)
            }
        });

        // Сортировка контейнеров внутри группы по имени
        for items in grouped.values_mut() {
            items.sort_by(|a, b| a.names.cmp(&b.names));
        }

        v_flex()
            .id("docker-panel")
            .track_focus(&self.focus_handle)
            .size_full()
            .p_3()
            .bg(cx.theme().colors().panel_background)
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(Label::new(" Docker").size(LabelSize::Large))
                    .child(
                        Button::new("refresh", "Refresh")
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
            )
            .child(
                Label::new(&self.status)
                    .color(Color::Muted)
                    .size(LabelSize::Small),
            )
            .child(
                div()
                    .id("docker-scroll-area")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .gap_3()
                            // ==================== CONTAINERS ====================
                            .child(
                                h_flex()
                                    .id("containers-header")
                                    .justify_between()
                                    .items_center()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|style| {
                                        style.bg(cx.theme().colors().ghost_element_hover)
                                    })
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.toggle_containers(cx)),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(
                                                Icon::new(if self.containers_expanded {
                                                    IconName::ChevronDown
                                                } else {
                                                    IconName::ChevronRight
                                                })
                                                .size(IconSize::Small),
                                            )
                                            .child(
                                                Label::new("Containers")
                                                    .size(LabelSize::Large)
                                                    .weight(FontWeight::MEDIUM),
                                            ),
                                    )
                                    .child(
                                        Label::new(self.containers.len().to_string())
                                            .size(LabelSize::Small)
                                            .color(Color::Muted),
                                    ),
                            )
                            .when(self.containers_expanded, |this| {
                                this.child(v_flex().gap_2().children(group_names.iter().map(
                                    |project_name| {
                                        let items = grouped.get(project_name).unwrap();
                                        let is_group_expanded = self
                                            .container_groups_expanded
                                            .get(project_name)
                                            .copied()
                                            .unwrap_or(false);

                                        v_flex()
                                            .gap_1()
                                            .child(
                                                h_flex()
                                                    .id(format!("group-{}", project_name))
                                                    .justify_between()
                                                    .items_center()
                                                    .px_2()
                                                    .py_1()
                                                    .rounded_md()
                                                    .hover(|style| {
                                                        style.bg(cx
                                                            .theme()
                                                            .colors()
                                                            .ghost_element_hover)
                                                    })
                                                    .on_click(cx.listener({
                                                        let p = project_name.clone();
                                                        move |this, _, _, cx| {
                                                            this.toggle_container_group(&p, cx);
                                                        }
                                                    }))
                                                    .child(
                                                        h_flex()
                                                            .gap_2()
                                                            .items_center()
                                                            .child(
                                                                Icon::new(if is_group_expanded {
                                                                    IconName::ChevronDown
                                                                } else {
                                                                    IconName::ChevronRight
                                                                })
                                                                .size(IconSize::Small),
                                                            )
                                                            .child(
                                                                Label::new(project_name.clone())
                                                                    .size(LabelSize::Small)
                                                                    .weight(FontWeight::SEMIBOLD),
                                                            ),
                                                    )
                                                    .child(
                                                        h_flex()
                                                            .gap_2()
                                                            .items_center()
                                                            .child(
                                                                Label::new(items.len().to_string())
                                                                    .size(LabelSize::Small)
                                                                    .color(Color::Muted),
                                                            )
                                                            .child(
                                                                IconButton::new(
                                                                    format!(
                                                                        "group-start-{}",
                                                                        project_name
                                                                    ),
                                                                    IconName::PlayFilled,
                                                                )
                                                                .icon_size(IconSize::Small)
                                                                .disabled(
                                                                    items.iter().all(|c| {
                                                                        c.state == "running"
                                                                    }),
                                                                )
                                                                .on_click(cx.listener({
                                                                    let p = project_name.clone();
                                                                    move |this, _, _, cx| {
                                                                        this.docker_group_action(
                                                                            "start", &p, cx,
                                                                        );
                                                                    }
                                                                })),
                                                            )
                                                            .child(
                                                                IconButton::new(
                                                                    format!(
                                                                        "group-stop-{}",
                                                                        project_name
                                                                    ),
                                                                    IconName::Stop,
                                                                )
                                                                .icon_size(IconSize::Small)
                                                                .disabled(
                                                                    !items.iter().any(|c| {
                                                                        c.state == "running"
                                                                    }),
                                                                )
                                                                .on_click(cx.listener({
                                                                    let p = project_name.clone();
                                                                    move |this, _, _, cx| {
                                                                        this.docker_group_action(
                                                                            "stop", &p, cx,
                                                                        );
                                                                    }
                                                                })),
                                                            ),
                                                    ),
                                            )
                                            .when(is_group_expanded, |this| {
                                                this.child(
                                                    v_flex().gap_2().pl_4().children(
                                                        items
                                                            .iter()
                                                            .map(|c| self.render_container(c, cx)),
                                                    ),
                                                )
                                            })
                                    },
                                )))
                            })
                            // ==================== IMAGES ====================
                            .child(
                                h_flex()
                                    .id("images-header")
                                    .justify_between()
                                    .items_center()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|style| {
                                        style.bg(cx.theme().colors().ghost_element_hover)
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_images(cx)))
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(
                                                Icon::new(if self.images_expanded {
                                                    IconName::ChevronDown
                                                } else {
                                                    IconName::ChevronRight
                                                })
                                                .size(IconSize::Small),
                                            )
                                            .child(
                                                Label::new("Images")
                                                    .size(LabelSize::Large)
                                                    .weight(FontWeight::MEDIUM),
                                            ),
                                    )
                                    .child(
                                        Label::new(self.images.len().to_string())
                                            .size(LabelSize::Small)
                                            .color(Color::Muted),
                                    ),
                            )
                            .when(self.images_expanded, |this| {
                                this.child(v_flex().gap_2().children(self.images.iter().map(
                                    |img| {
                                        let display_name =
                                            if img.tag == "<none>" || img.tag.is_empty() {
                                                img.repository.clone()
                                            } else {
                                                format!("{}:{}", img.repository, img.tag)
                                            };
                                        let short_id = img.id.chars().take(12).collect::<String>();

                                        v_flex()
                                            .id(format!("image-{}", short_id))
                                            .p_2()
                                            .rounded_md()
                                            .bg(cx.theme().colors().panel_background)
                                            .border_1()
                                            .border_color(cx.theme().colors().border)
                                            .gap_2()
                                            .child(
                                                Label::new(&display_name)
                                                    .weight(FontWeight::MEDIUM),
                                            )
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .child(
                                                        Label::new(&img.id)
                                                            .size(LabelSize::Small)
                                                            .color(Color::Muted),
                                                    )
                                                    .child(
                                                        Label::new(&img.size)
                                                            .size(LabelSize::Small)
                                                            .color(Color::Muted),
                                                    ),
                                            )
                                            .child(
                                                h_flex().gap_1().child(
                                                    IconButton::new(
                                                        format!("delete-image-{}", short_id),
                                                        IconName::Trash,
                                                    )
                                                    .icon_size(IconSize::Small)
                                                    .on_click(cx.listener({
                                                        let id = img.id.clone();
                                                        move |this, _, _, cx| {
                                                            this.remove_image(&id, cx);
                                                        }
                                                    })),
                                                ),
                                            )
                                    },
                                )))
                            })
                            // ==================== VOLUMES ====================
                            .child(
                                h_flex()
                                    .id("volumes-header")
                                    .justify_between()
                                    .items_center()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .hover(|style| {
                                        style.bg(cx.theme().colors().ghost_element_hover)
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_volumes(cx)))
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(
                                                Icon::new(if self.volumes_expanded {
                                                    IconName::ChevronDown
                                                } else {
                                                    IconName::ChevronRight
                                                })
                                                .size(IconSize::Small),
                                            )
                                            .child(
                                                Label::new("Volumes")
                                                    .size(LabelSize::Large)
                                                    .weight(FontWeight::MEDIUM),
                                            ),
                                    )
                                    .child(
                                        Label::new(self.volumes.len().to_string())
                                            .size(LabelSize::Small)
                                            .color(Color::Muted),
                                    ),
                            )
                            .when(self.volumes_expanded, |this| {
                                this.child(v_flex().gap_2().children(self.volumes.iter().map(
                                    |vol| {
                                        v_flex()
                                            .id(format!("volume-{}", vol.name))
                                            .p_2()
                                            .rounded_md()
                                            .bg(cx.theme().colors().panel_background)
                                            .border_1()
                                            .border_color(cx.theme().colors().border)
                                            .gap_2()
                                            .child(Label::new(&vol.name).weight(FontWeight::MEDIUM))
                                            .child(
                                                Label::new(&vol.driver)
                                                    .size(LabelSize::Small)
                                                    .color(Color::Muted),
                                            )
                                            .child(
                                                h_flex().gap_1().child(
                                                    IconButton::new(
                                                        format!("delete-volume-{}", vol.name),
                                                        IconName::Trash,
                                                    )
                                                    .icon_size(IconSize::Small)
                                                    .on_click(cx.listener({
                                                        let name = vol.name.clone();
                                                        move |this, _, _, cx| {
                                                            this.remove_volume(&name, cx);
                                                        }
                                                    })),
                                                ),
                                            )
                                    },
                                )))
                            }),
                    ),
            )
    }
}

impl Focusable for DockerPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for DockerPanel {}

impl Panel for DockerPanel {
    fn persistent_name() -> &'static str {
        "DockerPanel"
    }
    fn panel_key() -> &'static str {
        "DockerPanel"
    }
    fn position(&self, _w: &Window, _c: &App) -> DockPosition {
        DockPosition::Left
    }
    fn position_is_valid(&self, _p: DockPosition) -> bool {
        true
    }
    fn set_position(&mut self, _p: DockPosition, _w: &mut Window, _c: &mut Context<Self>) {}
    fn default_size(&self, _w: &Window, _c: &App) -> Pixels {
        px(340.)
    }
    fn icon(&self, _w: &Window, _c: &App) -> Option<IconName> {
        Some(IconName::Server)
    }
    fn icon_tooltip(&self, _w: &Window, _c: &App) -> Option<&'static str> {
        Some("Docker Containers")
    }
    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleDockerPanel) as Box<dyn Action>
    }
    fn activation_priority(&self) -> u32 {
        800
    }
}

actions!(docker_panel, [ToggleDockerPanel]);

pub fn init(cx: &mut App) {
    println!("DockerPanel init called!");

    cx.observe_new(|workspace: &mut Workspace, mut window, cx| {
        println!("Creating DockerPanel instance");
        workspace.register_action(
            |workspace: &mut Workspace, _action: &ToggleDockerPanel, window, cx| {
                workspace.toggle_panel_focus::<DockerPanel>(window, cx);
            },
        );

        let workspace_entity = cx.entity();
        let panel =
            cx.new(|cx| DockerPanel::new(cx, window.as_mut().unwrap(), workspace_entity.clone()));
        workspace.add_panel(panel, window.as_mut().unwrap(), cx);
    })
    .detach();
}
