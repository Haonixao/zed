use chrono::Local;
use collections::HashMap;
use gpui::AppContext;
use gpui::WeakEntity; // уже должен быть, но на всякий
use gpui::*;
use gpui::{App, AsyncApp, Context};
use project::Project;
use serde::Deserialize;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::Command;
use task::{
    HideStrategy, RevealStrategy, RevealTarget, SaveStrategy, Shell, SpawnInTerminal, TaskId,
};
use terminal::Terminal; // ← обязательно
use terminal_view::TerminalView;
use terminal_view::terminal_panel::TerminalPanel;
use ui::{Button, Color, Icon, IconButton, IconName, IconSize, Label, LabelSize, prelude::*};
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

#[derive(Debug, Clone, Deserialize)]
struct Container {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Names")]
    names: String,
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "State")]
    state: String,
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
}

impl DockerPanel {
    pub fn new(cx: &mut Context<Self>, _window: &mut Window, workspace: Entity<Workspace>) -> Self {
        let mut this = Self {
            containers: vec![],
            images: vec![],
            volumes: vec![],
            status: "Не обновлялось".to_string(),
            focus_handle: cx.focus_handle(),
            workspace: workspace.downgrade(),
            containers_expanded: true,
            images_expanded: false,
            volumes_expanded: false,
        };
        this.refresh(cx);
        this
    }

    fn docker_cmd() -> std::process::Command {
        let mut cmd = std::process::Command::new("docker");

        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000);
        }

        cmd
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

    fn toggle_containers(&mut self, cx: &mut Context<Self>) {
        self.containers_expanded = !self.containers_expanded;
        cx.notify();
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.status = format!("Обновление... {}", Local::now().format("%H:%M:%S"));

        // === Containers ===
        let output = DockerPanel::docker_cmd()
            .arg("ps")
            .arg("-a")
            .arg("--format")
            .arg("{{json .}}")
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                self.containers = text
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .filter_map(|line| serde_json::from_str::<Container>(line).ok())
                    .collect();
            }
            Ok(_) => self.status = "docker ps вернул ошибку".to_string(),
            Err(e) => self.status = format!("docker не найден: {}", e),
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
            _ => {} // можно добавить обработку ошибки позже
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
            _ => {} // можно добавить обработку ошибки позже
        }

        self.status = format!("Обновлено {}", Local::now().format("%H:%M:%S"));
        cx.notify();
    }

    fn docker_action(&mut self, action: &str, container_id: &str, cx: &mut Context<Self>) {
        let _ = DockerPanel::docker_cmd()
            .arg(action)
            .arg(container_id)
            .output();
        self.refresh(cx);
    }

    fn show_logs(&self, window: &mut Window, cx: &mut Context<Self>, container_id: &str) {
        let short_id = container_id.chars().take(12).collect::<String>();
        eprintln!(
            "🐳 [Docker Logs] show_logs вызван для контейнера: {}",
            short_id
        );
    
        let Some(workspace) = self.workspace.upgrade() else {
            eprintln!("❌ [Docker Logs] Не удалось upgrade workspace");
            return;
        };
    
        let spawn_task = SpawnInTerminal {
            id: TaskId(format!("docker-logs-{}", short_id)),
            full_label: format!("🐳 Logs — {}", short_id),
            label: format!("🐳 Logs — {}", short_id),
            command_label: format!("docker logs -f {}", short_id),
            command: Some("docker".into()),
            args: vec!["logs".into(), "-f".into(), container_id.into()],
            cwd: None,
            env: HashMap::default(),
            use_new_terminal: true,
            allow_concurrent_runs: true,
            reveal: RevealStrategy::Always,      // ← лучше Always при запуске из панели
            reveal_target: RevealTarget::Dock,
            hide: HideStrategy::Never,
            shell: Shell::System,
            show_summary: true,                  // ← чтобы видеть ошибки и exit code
            show_command: true,
            show_rerun: true,
            save: SaveStrategy::None,
        };
    
        eprintln!("✅ [Docker Logs] SpawnInTerminal создан");
    
        // Запускаем задачу и сразу получаем Task
        let task_handle = workspace.update(cx, |workspace, cx| {
            let _ = workspace.toggle_panel_focus::<TerminalPanel>(window, cx);
    
            if let Some(terminal_panel) = workspace.panel::<TerminalPanel>(cx) {
                terminal_panel.update(cx, |terminal_panel, cx| {
                    terminal_panel.add_terminal_task(
                        spawn_task,
                        RevealStrategy::Always,
                        window,
                        cx,
                    )
                })
            } else {
                // fallback
                Task::ready(Err(anyhow::anyhow!("TerminalPanel not found")))
            }
        });
    
        // Правильный spawn для GPUI
        cx.spawn({
            let task_handle = task_handle; // переносим в замыкание
            move |_this: WeakEntity<Self>, _cx: &mut AsyncApp| async move {
                match task_handle.await {
                    Ok(weak_terminal) => {
                        eprintln!("✅ [Docker Logs] Терминал успешно создан: {:?}", weak_terminal);
                    }
                    Err(e) => {
                        eprintln!("❌ [Docker Logs] Ошибка при создании терминала: {:?}", e);
                    }
                }
            }
        })
        .detach();
    
        eprintln!("🏁 [Docker Logs] show_logs завершён");
    }
}

impl Render for DockerPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child(Label::new("🐳 Docker").size(LabelSize::Large))
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
            // ==================== СКРОЛЛИРУЕМАЯ ОБЛАСТЬ ====================
            .child(
                div()
                    .id("docker-scroll-area")
                    .flex_1() // занимает всё оставшееся место
                    .overflow_y_scroll() // ← вот и весь скроллинг
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
                                this.child(v_flex().gap_2().children(self.containers.iter().map(
                                    |c| {
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
                                            .child(
                                                h_flex().justify_between().items_center().child(
                                                    h_flex()
                                                        .gap_2()
                                                        .items_center()
                                                        .child(
                                                            Label::new(&c.names)
                                                                .weight(FontWeight::MEDIUM),
                                                        )
                                                        .child(
                                                            Label::new(if is_running {
                                                                "R"
                                                            } else {
                                                                "S"
                                                            })
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
                                            .child(
                                                Label::new(&c.image)
                                                    .size(LabelSize::Small)
                                                    .color(Color::Muted),
                                            )
                                            .child(
                                                h_flex()
                                                    .gap_1()
                                                    .child(
                                                        IconButton::new(
                                                            format!("start-{}", short_id),
                                                            IconName::PlayFilled,
                                                        )
                                                        .icon_size(IconSize::Small)
                                                        .disabled(is_running)
                                                        .on_click(cx.listener({
                                                            let id = c.id.clone();
                                                            move |this, _, _, cx| {
                                                                this.docker_action(
                                                                    "start",
                                                                    id.as_str(),
                                                                    cx,
                                                                );
                                                            }
                                                        })),
                                                    )
                                                    .child(
                                                        IconButton::new(
                                                            format!("stop-{}", short_id),
                                                            IconName::Stop,
                                                        )
                                                        .icon_size(IconSize::Small)
                                                        .disabled(!is_running)
                                                        .on_click(cx.listener({
                                                            let id = c.id.clone();
                                                            move |this, _, _, cx| {
                                                                this.docker_action(
                                                                    "stop",
                                                                    id.as_str(),
                                                                    cx,
                                                                );
                                                            }
                                                        })),
                                                    )
                                                    .child(
                                                        IconButton::new(
                                                            format!("restart-{}", short_id),
                                                            IconName::RotateCw,
                                                        )
                                                        .icon_size(IconSize::Small)
                                                        .on_click(cx.listener({
                                                            let id = c.id.clone();
                                                            move |this, _, _, cx| {
                                                                this.docker_action(
                                                                    "restart",
                                                                    id.as_str(),
                                                                    cx,
                                                                );
                                                            }
                                                        })),
                                                    )
                                                    .child(
                                                        IconButton::new(
                                                            format!("logs-{}", short_id),
                                                            IconName::Notepad,
                                                        )
                                                        .icon_size(IconSize::Small)
                                                        .on_click(cx.listener({
                                                            let id = c.id.clone();
                                                            move |this, _, window, cx| {
                                                                this.show_logs(window, cx, &id);
                                                            }
                                                        })),
                                                    )
                                                    .child(
                                                        IconButton::new(
                                                            format!("remove-{}", short_id),
                                                            IconName::Trash,
                                                        )
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
                                                // Название образа
                                                Label::new(&display_name)
                                                    .weight(FontWeight::MEDIUM),
                                            )
                                            .child(
                                                // ID + размер
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
                                                // Строка кнопок (как у контейнеров)
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
                                        let short_name = if vol.name.len() > 20 {
                                            format!("{}…", &vol.name[..17])
                                        } else {
                                            vol.name.clone()
                                        };

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
        Box::new(ToggleDockerPanel)
    }
    fn activation_priority(&self) -> u32 {
        800
    }
}

actions!(docker_panel, [ToggleDockerPanel]);

pub fn init(cx: &mut App) {
    println!("🚀 DockerPanel init called!");

    cx.observe_new(|workspace: &mut Workspace, mut window, cx| {
        println!("📦 Creating DockerPanel instance");
        workspace.register_action(
            |workspace: &mut Workspace, _action: &ToggleDockerPanel, window, cx| {
                workspace.toggle_panel_focus::<DockerPanel>(window, cx);
            },
        );

        let workspace_entity = cx.entity(); // ← текущий workspace
        let panel =
            cx.new(|cx| DockerPanel::new(cx, window.as_mut().unwrap(), workspace_entity.clone()));
        workspace.add_panel(panel, window.as_mut().unwrap(), cx);
    })
    .detach();
}
