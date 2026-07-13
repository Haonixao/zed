use gpui::WeakEntity;
use gpui::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use ui::{Button, IconButton, IconName, IconSize, Label, LabelSize, prelude::*};
use ui_input::InputField;
use uuid::Uuid;

use crate::models::*;
use base64::prelude::*;
use futures_lite::io::AsyncReadExt;
use http_client::{AsyncBody, HttpClient, Method, Request};
use serde_json::json;

use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

struct HtmlThemeColors {
    background: String,
    editor_background: String,
    border: String,
    text: String,
    accent: String,
}

fn hsla_to_hex(hsla: gpui::Hsla) -> String {
    let h = hsla.h;
    let s = hsla.s;
    let l = hsla.l;

    if s == 0.0 {
        let gray = (l * 255.0) as u8;
        return format!("#{:02x}{:02x}{:02x}", gray, gray, gray);
    }

    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;

    fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
        if t < 0.0 { t += 1.0; }
        if t > 1.0 { t -= 1.0; }
        if t < 1.0/6.0 { return p + (q - p) * 6.0 * t; }
        if t < 1.0/2.0 { return q; }
        if t < 2.0/3.0 { return p + (q - p) * (2.0/3.0 - t) * 6.0; }
        p
    }

    let r = hue_to_rgb(p, q, h + 1.0/3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0/3.0);

    let ri = (r * 255.0) as u8;
    let gi = (g * 255.0) as u8;
    let bi = (b * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", ri, gi, bi)
}

fn generate_interactive_html_table(table_name: &str, columns: &[ColumnInfo], data: &[serde_json::Value], colors: &HtmlThemeColors) -> String {
    let headers: Vec<String> = columns.iter().map(|c| escape_html(&c.name)).collect();

    let header_html = format!("<th>#</th>{}", headers.iter().map(|h| format!("<th>{}</th>", h)).collect::<Vec<_>>().join(""));
    let header_row = format!("<tr>{}</tr>", header_html);

    let rows_html: Vec<String> = data.iter().enumerate().map(|(idx, row)| {
        let row_num = idx + 1;
        let cells: Vec<String> = columns.iter().map(|col| {
            let value = row.get(&col.name)
                .map(|v| {
                    if col.data_type == "bytea" {
                        "[bytea data]".to_string()
                    } else {
                        match v {
                            serde_json::Value::String(s) => s.clone(),
                            serde_json::Value::Null => "NULL".to_string(),
                            other => other.to_string(),
                        }
                    }
                })
                .unwrap_or_default();
            format!("<td>{}</td>", escape_html(&value))
        }).collect();
        format!(r#"<tr class="data-row" data-row="{}">
            <td class="row-num">{}</td>
            {}
        </tr>
        <tr class="detail-row" id="detail-{}">
            <td colspan="{}">
                <div class="detail-content">
                    {}
                </div>
            </td>
        </tr>"#,
            row_num,
            row_num,
            cells.join(""),
            row_num,
            columns.len() + 1,
            columns.iter().map(|col| {
                let value = row.get(&col.name)
                    .map(|v| {
                        if col.data_type == "bytea" {
                            "[bytea data]".to_string()
                        } else {
                            match v {
                                serde_json::Value::String(s) => s.clone(),
                                serde_json::Value::Null => "NULL".to_string(),
                                other => other.to_string(),
                            }
                        }
                    })
                    .unwrap_or_default();
                format!(r#"<div class="detail-item"><span class="detail-label">{}:</span> <span class="detail-value">{}</span></div>"#,
                    escape_html(&col.name), escape_html(&value))
            }).collect::<Vec<_>>().join("")
        )
    }).collect();
    let tbody = rows_html.join("");

    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{table_name} - SQL Table</title>
    <style>
        * {{ box-sizing: border-box; margin: 0; padding: 0; }}
        body {{
            font-family: system-ui, sans-serif;
            background: {background};
            color: {text};
            padding: 20px;
            height: calc(100vh - 40px);
            display: flex;
            flex-direction: column;
        }}
        h1 {{
            color: {accent};
            margin-bottom: 20px;
            font-size: 1.5rem;
            flex-shrink: 0;
        }}
        .table-container {{
            flex: 1;
            overflow: auto;
            border-radius: 8px;
            box-shadow: 0 4px 6px rgba(0, 0, 0, 0.3);
        }}
        ::-webkit-scrollbar {{
            width: 10px;
            height: 10px;
        }}
        ::-webkit-scrollbar-track {{
            background: {border};
            border-radius: 5px;
        }}
        ::-webkit-scrollbar-thumb {{
            background: {accent};
            border-radius: 5px;
        }}
        ::-webkit-scrollbar-thumb:hover {{
            background: {text};
        }}
        ::-webkit-scrollbar-corner {{
            background: {border};
        }}
        table {{
            width: 100%;
            border-collapse: collapse;
            background: {editor_background};
        }}
        th, td {{
            padding: 10px 12px;
            text-align: left;
            border-bottom: 1px solid {border};
        }}
        th {{
            background: {editor_background};
            color: {accent};
            font-weight: 600;
            cursor: pointer;
            user-select: none;
            position: sticky;
            top: 0;
            z-index: 10;
        }}
        th:hover {{
            background: {border};
        }}
        th::after {{
            content: '';
            display: inline-block;
            margin-left: 8px;
            opacity: 0.5;
        }}
        th.asc::after {{
            content: '▲';
        }}
        th.desc::after {{
            content: '▼';
        }}
        tr.data-row:hover {{
            background: {border};
            cursor: pointer;
        }}
        td {{
            max-width: 300px;
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
        }}
        .row-num {{
            color: {accent};
            font-weight: 600;
            width: 50px;
            text-align: center;
            cursor: pointer;
        }}
        .row-num:hover {{
            background: rgba(0,0,0,0.1);
        }}
        tr.detail-row {{
            display: none;
        }}
        tr.detail-row.open {{
            display: table-row;
        }}
        tr.detail-row td {{
            padding: 0;
            background: {background};
            border-bottom: 2px solid {border};
        }}
        .detail-content {{
            padding: 16px 20px;
            display: flex;
            flex-wrap: wrap;
            gap: 12px;
        }}
        .detail-item {{
            min-width: 200px;
            padding: 8px 12px;
            background: {editor_background};
            border-radius: 6px;
            border: 1px solid {border};
        }}
        .detail-label {{
            color: {accent};
            font-weight: 600;
            display: block;
            margin-bottom: 4px;
        }}
        .detail-value {{
            word-break: break-all;
            white-space: pre-wrap;
        }}
        .info {{
            margin-top: 15px;
            color: {accent};
            font-size: 0.9rem;
        }}
    </style>
</head>
<body>
    <h1>📋 {table_name}</h1>
    <div class="table-container">
        <table>
            <thead>
                {header_row}
            </thead>
            <tbody id="table-body">
                {tbody}
            </tbody>
        </table>
    </div>
    <p class="info">💡 Click row number to expand | Click column header to sort</p>

    <script>
        let sortCol = null;
        let sortAsc = true;

        document.querySelectorAll('.row-num').forEach(cell => {{
            cell.addEventListener('click', (e) => {{
                e.stopPropagation();
                const row = cell.closest('tr');
                const rowNum = row.getAttribute('data-row');
                const detailRow = document.getElementById('detail-' + rowNum);
                if (detailRow) {{
                    detailRow.classList.toggle('open');
                }}
            }});
        }});

        document.querySelectorAll('th').forEach((th, i) => {{
            if (i === 0) return;
            th.addEventListener('click', () => {{
                document.querySelectorAll('th').forEach(h => {{ h.classList.remove('asc', 'desc'); }});

                if (sortCol === i) {{
                    sortAsc = !sortAsc;
                }} else {{
                    sortAsc = true;
                    sortCol = i;
                }}

                th.classList.add(sortAsc ? 'asc' : 'desc');

                const tbody = document.getElementById('table-body');
                const pairs = [];

                document.querySelectorAll('.data-row').forEach(dataRow => {{
                    const rowNum = dataRow.getAttribute('data-row');
                    const detailRow = document.getElementById('detail-' + rowNum);
                    pairs.push({{ dataRow, detailRow }});
                }});

                pairs.sort((a, b) => {{
                    const aVal = a.dataRow.cells[i].textContent;
                    const bVal = b.dataRow.cells[i].textContent;
                    const aNum = parseFloat(aVal);
                    const bNum = parseFloat(bVal);

                    let result;
                    if (!isNaN(aNum) && !isNaN(bNum)) {{
                        result = sortAsc ? aNum - bNum : bNum - aNum;
                    }} else {{
                        result = sortAsc ? aVal.localeCompare(bVal) : bVal.localeCompare(aVal);
                    }}
                    return result;
                }});

                pairs.forEach((pair, newIdx) => {{
                    const newRowNum = newIdx + 1;
                    const oldRowNum = pair.dataRow.getAttribute('data-row');

                    if (oldRowNum !== String(newRowNum)) {{
                        pair.dataRow.setAttribute('data-row', newRowNum);
                        pair.dataRow.querySelector('.row-num').textContent = newRowNum;

                        pair.detailRow.id = 'detail-' + newRowNum;
                    }}

                    tbody.appendChild(pair.dataRow);
                    if (pair.detailRow) {{
                        tbody.appendChild(pair.detailRow);
                    }}
                }});
            }});
        }});
    </script>
</body>
</html>"#,
        table_name = escape_html(table_name),
        header_row = header_row,
        tbody = tbody,
        background = colors.background,
        editor_background = colors.editor_background,
        border = colors.border,
        text = colors.text,
        accent = colors.accent
    )
}

pub struct SqlApiExplorerPanel {
    focus_handle: FocusHandle,
    workspace: WeakEntity<Workspace>,
    hosts: Vec<HostConfig>,
    status: String,

    // Форма добавления
    show_add_form: bool,
    new_host_input: Option<Entity<InputField>>,
    new_bearer_token_input: Option<Entity<InputField>>,
    new_header_name_input: Option<Entity<InputField>>,
    new_header_value_input: Option<Entity<InputField>>,
    new_username_input: Option<Entity<InputField>>,
    new_password_input: Option<Entity<InputField>>,
    new_auth_type: String,

    // Tree state
    expanded_hosts: HashMap<String, bool>,
    schemas: HashMap<String, Vec<String>>,
    loading_hosts: HashSet<String>,

    tables: HashMap<String, HashMap<String, Vec<String>>>, // host → schema → таблицы
    expanded_schemas: HashMap<String, HashSet<String>>,    // host → множество раскрытых схем
    loading_tables: HashSet<String>,                       // composite key "host|schema"
}

impl SqlApiExplorerPanel {
    pub fn new(cx: &mut Context<Self>, workspace: Entity<Workspace>) -> Self {
        let hosts = SqlApiHosts::load()
            .map(|h| h.hosts)
            .unwrap_or_else(|_| cx.global::<SqlApiHosts>().hosts.clone());

        let mut this = Self {
            focus_handle: cx.focus_handle(),
            workspace: workspace.downgrade(),
            hosts,
            status: "Ready to work".to_string(),

            show_add_form: false,
            new_host_input: None,
            new_bearer_token_input: None,
            new_header_name_input: None,
            new_header_value_input: None,
            new_username_input: None,
            new_password_input: None,
            new_auth_type: "none".to_string(),

            expanded_hosts: HashMap::new(),
            schemas: HashMap::new(),
            loading_hosts: HashSet::new(),

            tables: HashMap::new(),
            expanded_schemas: HashMap::new(),
            loading_tables: HashSet::new(),
        };

        if this.hosts.is_empty() {
            this.hosts.push(HostConfig::new("https://example.com".to_string()));
        }
        this.save_hosts(cx);

        this
    }

    fn save_hosts(&self, cx: &mut Context<Self>) {
        cx.global_mut::<SqlApiHosts>().hosts.clone_from(&self.hosts);
        let hosts_to_save = SqlApiHosts { hosts: self.hosts.clone() };
        if let Err(e) = hosts_to_save.save() {
            eprintln!("Failed to save hosts: {}", e);
        }
    }

    // Создаём/обновляем все нужные InputField при открытии формы
    fn ensure_input_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.new_host_input.is_none() {
            self.new_host_input = Some(cx.new(|cx| InputField::new(window, cx, "https://")));
        }

        if self.new_bearer_token_input.is_none() {
            self.new_bearer_token_input = Some(cx.new(|cx| InputField::new(window, cx, "")));
        }
        if self.new_header_name_input.is_none() {
            self.new_header_name_input =
                Some(cx.new(|cx| InputField::new(window, cx, "X-API-Key")));
        }
        if self.new_header_value_input.is_none() {
            self.new_header_value_input = Some(cx.new(|cx| InputField::new(window, cx, "")));
        }
        if self.new_username_input.is_none() {
            self.new_username_input = Some(cx.new(|cx| InputField::new(window, cx, "")));
        }
        if self.new_password_input.is_none() {
            self.new_password_input = Some(cx.new(|cx| InputField::new(window, cx, "")));
        }
    }

    fn toggle_add_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_add_form = !self.show_add_form;
        if self.show_add_form {
            self.ensure_input_fields(window, cx);
        }
        cx.notify();
    }

    fn close_add_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_add_form = false;
        self.new_auth_type = "none".to_string();

        // Сбрасываем поля
        if let Some(input) = &self.new_host_input {
            input.update(cx, |i, cx| i.set_text("https://", window, cx));
        }
        cx.notify();
    }

    fn set_auth_type(&mut self, ty: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.new_auth_type = ty.to_string();
        self.ensure_input_fields(window, cx); // на всякий случай
        cx.notify();
    }

    fn add_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let host_url = self
            .new_host_input
            .as_ref()
            .map(|i| i.read(cx).text(cx).trim().to_string())
            .unwrap_or_default();

        if host_url.is_empty() {
            self.status = "❌ Enter host URL".to_string();
            cx.notify();
            return;
        }

        let auth = match self.new_auth_type.as_str() {
            "bearer" => {
                let token = self
                    .new_bearer_token_input
                    .as_ref()
                    .map(|i| i.read(cx).text(cx).trim().to_string())
                    .filter(|s| !s.is_empty());
                Some(AuthConfig {
                    r#type: "bearer".to_string(),
                    token,
                    header_name: None,
                    header_value: None,
                    username: None,
                    password: None,
                })
            }
            "custom-header" => {
                let header_name = self
                    .new_header_name_input
                    .as_ref()
                    .map(|i| i.read(cx).text(cx).trim().to_string())
                    .filter(|s| !s.is_empty());
                let header_value = self
                    .new_header_value_input
                    .as_ref()
                    .map(|i| i.read(cx).text(cx).trim().to_string())
                    .filter(|s| !s.is_empty());
                Some(AuthConfig {
                    r#type: "custom-header".to_string(),
                    token: None,
                    header_name,
                    header_value,
                    username: None,
                    password: None,
                })
            }
            "basic" => {
                let username = self
                    .new_username_input
                    .as_ref()
                    .map(|i| i.read(cx).text(cx).trim().to_string())
                    .filter(|s| !s.is_empty());
                let password = self
                    .new_password_input
                    .as_ref()
                    .map(|i| i.read(cx).text(cx).trim().to_string());
                Some(AuthConfig {
                    r#type: "basic".to_string(),
                    token: None,
                    header_name: None,
                    header_value: None,
                    username,
                    password,
                })
            }
            _ => None,
        };

        let new_host = HostConfig {
            name: host_url,
            auth,
        };

        self.hosts.push(new_host);
        self.save_hosts(cx);
        self.status = format!("✅ Host added ({} all)", self.hosts.len());
        self.close_add_form(window, cx);
        cx.notify();
    }

    // === Tree logic ===
    fn toggle_host(&mut self, host_name: String, _window: &mut Window, cx: &mut Context<Self>) {
        let expanded = self
            .expanded_hosts
            .entry(host_name.clone())
            .or_insert(false);
        *expanded = !*expanded;

        if *expanded
            && !self.schemas.contains_key(&host_name)
            && !self.loading_hosts.contains(&host_name)
        {
            self.fetch_schemas(host_name, cx);
        }
        cx.notify();
    }

    fn fetch_schemas(&mut self, host_name: String, cx: &mut Context<Self>) {
        self.loading_hosts.insert(host_name.clone());
        cx.notify();

        if let Some(host) = self.hosts.iter().find(|h| h.name == host_name).cloned() {
            let host_name_clone = host_name.clone();
            let client: Arc<dyn HttpClient> = cx.http_client();

            // Modern GPUI pattern (async move closure + WeakEntity::update)
            cx.spawn(async move |this, cx| {
                let result = Self::query_schemas_real(&host, client).await;

                let _ = this.update(cx, |panel: &mut Self, cx: &mut Context<Self>| {
                    match result {
                        Ok(schemas) => {
                            panel.schemas.insert(host_name_clone.clone(), schemas);
                        }
                        Err(err) => {
                            panel.status = format!("❌ {}", err);
                        }
                    }
                    panel.loading_hosts.remove(&host_name_clone);
                    cx.notify();
                });
            })
            .detach();
        } else {
            self.loading_hosts.remove(&host_name);
            cx.notify();
        }
    }

    async fn query_schemas_real(
        host: &HostConfig,
        client: Arc<dyn HttpClient>,
    ) -> Result<Vec<String>, String> {
        let sql = r#"
            SELECT nspname AS schema_name
            FROM pg_catalog.pg_namespace
            WHERE nspname !~ '^pg_'
            AND nspname <> 'information_schema'
            ORDER BY nspname
        "#;

        let url = format!("{}/api/v1/dev/query-sql", host.name.trim_end_matches('/'));

        let mut builder = Request::builder()
            .method(Method::POST)
            .uri(url)
            .header("Content-Type", "application/json");

        // Авторизация
        if let Some(auth) = &host.auth {
            match auth.r#type.as_str() {
                "bearer" => {
                    if let Some(token) = &auth.token {
                        builder = builder.header("Authorization", format!("Bearer {}", token));
                    }
                }
                "custom-header" => {
                    if let (Some(name), Some(value)) = (&auth.header_name, &auth.header_value) {
                        builder = builder.header(name, value);
                    }
                }
                "basic" => {
                    if let (Some(username), Some(password)) = (&auth.username, &auth.password) {
                        let credentials = format!("{}:{}", username, password);
                        let encoded = BASE64_STANDARD.encode(credentials.as_bytes());
                        builder = builder.header("Authorization", format!("Basic {}", encoded));
                    }
                }
                _ => {}
            }
        }

        let body = json!({ "sql": sql }).to_string();

        let request = builder
            .body(AsyncBody::from(body.into_bytes()))
            .map_err(|e| format!("Failed to build request: {}", e))?;

        let response = client
            .send(request)
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        // Правильное чтение тела ответа
        let mut body = response.into_body();
        let mut text = String::new();
        body.read_to_string(&mut text)
            .await
            .map_err(|e| format!("Failed to read response body: {}", e))?;

        let json_response: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("JSON parse error: {}", e))?;

        let data = json_response
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or("No 'data' field in response")?;

        let schemas: Vec<String> = data
            .iter()
            .filter_map(|row| {
                row.get("schema_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect();

        Ok(schemas)
    }

    fn toggle_schema(&mut self, host_name: String, schema: String, _window: &mut Window, cx: &mut Context<Self>) {
        let expanded = self
            .expanded_schemas
            .entry(host_name.clone())
            .or_default();

        let was_expanded = expanded.contains(&schema);

        if was_expanded {
            expanded.remove(&schema);
        } else {
            expanded.insert(schema.clone());

            // Загружаем таблицы, если ещё нет
            let key = format!("{}|{}", host_name, schema);
            if !self.loading_tables.contains(&key)
                && !self.tables.get(&host_name).and_then(|m| m.get(&schema)).is_some()
            {
                self.fetch_tables(host_name.clone(), schema.clone(), cx);
            }
        }
        cx.notify();
    }

    fn open_table_data(
        &mut self,
        host_name: String,
        schema: String,
        table: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            self.status = "❌ Workspace unavailable".to_string();
            cx.notify();
            return;
        };

        if let Some(host_config) = self.hosts.iter().find(|h| h.name == host_name).cloned() {
            let schema_clone = schema.clone();
            let table_clone = table.clone();
            let host_config_clone = host_config.clone();

            workspace.update(cx, |workspace, cx| {
                let item = cx.new(|cx| {
                    SqlTableDataView::new(host_config_clone, schema_clone, table_clone, cx)
                });

                workspace.add_item_to_active_pane(
                    Box::new(item) as Box<dyn workspace::ItemHandle>,
                    None,
                    true,
                    window,
                    cx,
                );
            });

            self.status = format!("📋 Table is opened: {}.{}", schema, table);
        } else {
            self.status = format!("❌ Host {} not found", host_name);
        }

        cx.notify();
    }

    fn open_table_ddl(
        &mut self,
        host_name: String,
        schema: String,
        table: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host_config) = self.hosts.iter().find(|h| h.name == host_name).cloned() else {
            self.status = format!("❌ Host {} not found", host_name);
            cx.notify();
            return;
        };

        let full_name = format!("DDL: {}.{}", schema, table);
        let loading_text = "⏳ DDL generation for table...\n\nWait...".to_string();

        let ddl_viewer = cx.new(|cx| DdlViewer::new(full_name.clone(), loading_text, cx));
        let ddl_viewer_weak = ddl_viewer.downgrade();

        // Открываем вкладку
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                workspace.add_item_to_active_pane(
                    Box::new(ddl_viewer) as Box<dyn workspace::ItemHandle>,
                    None,
                    true,
                    window,
                    cx,
                );
            });
        }

        self.status = format!("✅ DDL generation for {}.{} started", schema, table);
        cx.notify();

        // Асинхронная генерация (без this.update — безопасно!)
        let client: Arc<dyn HttpClient> = cx.http_client();

        cx.spawn(async move |_, cx| {
            let result = SqlTableDataView::fetch_ddl_real(&host_config, &schema, &table, client).await;

            let ddl_text = match result {
                Ok(ddl) => ddl,
                Err(err) => format!("❌ DDL generation ERROR:\n\n{}", err),
            };

            let _ = ddl_viewer_weak.update(cx, |viewer, cx| {
                viewer.update_ddl(ddl_text, cx);
            });
        })
        .detach();
    }

    fn fetch_tables(&mut self, host_name: String, schema: String, cx: &mut Context<Self>) {
        let key = format!("{}|{}", host_name, schema);
        self.loading_tables.insert(key.clone());
        cx.notify();

        if let Some(host) = self.hosts.iter().find(|h| h.name == host_name).cloned() {
            let host_name_clone = host_name.clone();
            let schema_clone = schema.clone();
            let client: Arc<dyn HttpClient> = cx.http_client();

            cx.spawn(async move |this, cx| {
                let result = Self::query_tables_real(&host, &schema_clone, client).await;

                let _ = this.update(cx, |panel: &mut Self, cx: &mut Context<Self>| {
                    match result {
                        Ok(tables) => {
                            panel.tables
                                .entry(host_name_clone.clone())
                                .or_default()
                                .insert(schema_clone.clone(), tables);
                        }
                        Err(err) => {
                            panel.status = format!("❌ {}", err);
                        }
                    }
                    panel.loading_tables.remove(&format!("{}|{}", host_name_clone, schema_clone));
                    cx.notify();
                });
            })
            .detach();
        } else {
            self.loading_tables.remove(&key);
            cx.notify();
        }
    }

    async fn query_tables_real(
        host: &HostConfig,
        schema: &str,
        client: Arc<dyn HttpClient>,
    ) -> Result<Vec<String>, String> {
        let sql = format!(
            r#"
                SELECT table_name
                FROM information_schema.tables
                WHERE table_schema = '{}'
                ORDER BY table_name
            "#,
            schema.replace("'", "''") // простая защита
        );

        let url = format!("{}/api/v1/dev/query-sql", host.name.trim_end_matches('/'));

        let mut builder = Request::builder()
            .method(Method::POST)
            .uri(url)
            .header("Content-Type", "application/json");

        // Авторизация (точно как в query_schemas_real)
        if let Some(auth) = &host.auth {
            match auth.r#type.as_str() {
                "bearer" => {
                    if let Some(token) = &auth.token {
                        builder = builder.header("Authorization", format!("Bearer {}", token));
                    }
                }
                "custom-header" => {
                    if let (Some(name), Some(value)) = (&auth.header_name, &auth.header_value) {
                        builder = builder.header(name, value);
                    }
                }
                "basic" => {
                    if let (Some(username), Some(password)) = (&auth.username, &auth.password) {
                        let credentials = format!("{}:{}", username, password);
                        let encoded = BASE64_STANDARD.encode(credentials.as_bytes());
                        builder = builder.header("Authorization", format!("Basic {}", encoded));
                    }
                }
                _ => {}
            }
        }

        let body = json!({ "sql": sql }).to_string();

        let request = builder
            .body(AsyncBody::from(body.into_bytes()))
            .map_err(|e| format!("Failed to build request: {}", e))?;

        let response = client
            .send(request)
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        let mut body = response.into_body();
        let mut text = String::new();
        body.read_to_string(&mut text)
            .await
            .map_err(|e| format!("Failed to read response body: {}", e))?;

        let json_response: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("JSON parse error: {}", e))?;

        let data = json_response
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or("No 'data' field in response")?;

        let tables: Vec<String> = data
            .iter()
            .filter_map(|row| {
                row.get("table_name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .collect();

        Ok(tables)
    }

    fn delete_host(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.hosts.len() {
            let name = self.hosts[index].name.clone();
            self.hosts.remove(index);
            self.expanded_hosts.remove(&name);
            self.schemas.remove(&name);
            self.tables.remove(&name);
            self.expanded_schemas.remove(&name);
            self.loading_hosts.remove(&name);
            self.loading_tables.retain(|k| !k.starts_with(&format!("{}|", name)));
            self.save_hosts(cx);
            self.status = "🗑️ Host deleted".to_string();
            cx.notify();
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.status = "🔄 Updated".to_string();
        cx.notify();
    }
}

impl Render for SqlApiExplorerPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("sql-api-explorer")
            .track_focus(&self.focus_handle)
            .size_full()
            .overflow_y_scroll() // ← вот главный фикс
            .p_3()
            .bg(cx.theme().colors().panel_background)
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(Label::new("🗄️ SQL API Explorer").size(LabelSize::Large))
                    .child(
                        Button::new("refresh", "Refresh")
                            .on_click(cx.listener(|this, _, _window, cx| this.refresh(cx))),
                    ),
            )
            .child(
                Label::new(&self.status)
                    .color(Color::Muted)
                    .size(LabelSize::Small),
            )
            // Список хостов (Tree)
            .child(
                v_flex()
                    .gap_2()
                    .children(self.hosts.iter().enumerate().map(|(index, host)| {
                        let host_name = host.name.clone();
                        let is_expanded = *self.expanded_hosts.get(&host_name).unwrap_or(&false);
                        let schemas = self.schemas.get(&host_name).cloned().unwrap_or_default();

                        v_flex()
                            .child(
                                div()
                                    .id(host_name.clone())
                                    .flex()
                                    .justify_between()
                                    .items_center()
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .hover(|s| s.bg(cx.theme().colors().title_bar_background))
                                    .on_click(cx.listener({
                                        let host_name = host_name.clone();
                                        move |this, _, window, cx| {
                                            this.toggle_host(host_name.clone(), window, cx);
                                        }
                                    }))
                                    .child(
                                        h_flex()
                                            .gap_3()
                                            .items_center()
                                            .child(
                                                Icon::new(if is_expanded {
                                                    IconName::ChevronDown
                                                } else {
                                                    IconName::ChevronRight
                                                })
                                                .size(IconSize::Small),
                                            )
                                            .child(
                                                Icon::new(IconName::Server).size(IconSize::Medium),
                                            )
                                            .child(
                                                Label::new(&host.name).weight(FontWeight::MEDIUM),
                                            ),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                Label::new(host.auth_summary())
                                                    .size(LabelSize::Small)
                                                    .color(Color::Muted),
                                            )
                                            .child(
                                                IconButton::new(
                                                    format!("delete-{}", index),
                                                    IconName::Trash,
                                                )
                                                .icon_size(IconSize::Small)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.delete_host(index, cx)
                                                })),
                                            ),
                                    ),
                            )
                            // Содержимое (схемы)
                            .when(is_expanded, |this| {
                                this.child(
                                    v_flex()
                                        .pl_8()
                                        .gap_1()
                                        .py_1()
                                        .children(schemas.into_iter().map(|schema| {
                                            let host_key = host_name.clone();
                                            let is_schema_expanded = self
                                                .expanded_schemas
                                                .get(&host_key)
                                                .map_or(false, |set| set.contains(&schema));

                                            let tables_for_schema = self
                                                .tables
                                                .get(&host_key)
                                                .and_then(|m| m.get(&schema))
                                                .cloned()
                                                .unwrap_or_default();

                                            let loading_key = format!("{}|{}", host_key, schema);
                                            let is_loading_table = self.loading_tables.contains(&loading_key);

                                            v_flex()
                                                .child(
                                                    div()
                                                        .id(format!("schema-{}", schema))
                                                        .flex()
                                                        .justify_between()
                                                        .items_center()
                                                        .px_3()
                                                        .py_1()
                                                        .rounded_md()
                                                        .hover(|s| s.bg(cx.theme().colors().title_bar_background))
                                                        .on_click(cx.listener({
                                                            let host_name = host_name.clone();
                                                            let schema = schema.clone();
                                                            move |this, _, window, cx| {
                                                                this.toggle_schema(host_name.clone(), schema.clone(), window, cx);
                                                            }
                                                        }))
                                                        .child(
                                                            h_flex()
                                                                .gap_3()
                                                                .items_center()
                                                                .child(
                                                                    Icon::new(if is_schema_expanded {
                                                                        IconName::ChevronDown
                                                                    } else {
                                                                        IconName::ChevronRight
                                                                    })
                                                                    .size(IconSize::Small),
                                                                )
                                                                .child(Icon::new(IconName::Folder).size(IconSize::Small))
                                                                .child(Label::new(&schema).size(LabelSize::Small)),
                                                        )
                                                )
                                                // Таблицы под схемой
                                                .when(is_schema_expanded, |this| {
                                                    this.child(
                                                        v_flex()
                                                            .pl_8()
                                                            .gap_1()
                                                            .py_1()
                                                            .child(if is_loading_table {
                                                                Label::new("⏳ Loading tables...")
                                                                    .color(Color::Muted)
                                                                    .into_any_element()
                                                            } else if tables_for_schema.is_empty() {
                                                                Label::new("No tables")
                                                                    .color(Color::Muted)
                                                                    .italic()
                                                                    .into_any_element()
                                                            } else {
                                                                v_flex()
                                                                    .gap_1()
                                                                    .children(tables_for_schema.into_iter().map(|table| {let host_name_clone = host_name.clone();
                                                                        let schema_clone = schema.clone();
                                                                        let table_clone = table.clone();

                                                                        div()
                                                                            .id(format!("table-{}-{}-{}",
                                                                                host_name.replace(|c: char| !c.is_alphanumeric(), "_"),
                                                                                schema,
                                                                                table
                                                                            ))
                                                                            .flex()
                                                                            .justify_between()
                                                                            .items_center()
                                                                            .px_3()
                                                                            .py_1()
                                                                            .rounded_md()
                                                                            .hover(|s| s.bg(cx.theme().colors().title_bar_background))
                                                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                                                this.open_table_data(
                                                                                    host_name_clone.clone(),
                                                                                    schema_clone.clone(),
                                                                                    table_clone.clone(),
                                                                                    window,
                                                                                    cx,
                                                                                );
                                                                            }))
                                                                            .on_mouse_down(
                                                                                    MouseButton::Right,
                                                                                    cx.listener({
                                                                                        let host_name_clone = host_name.clone();
                                                                                        let schema_clone = schema.clone();
                                                                                        let table_clone = table.clone();
                                                                                        move |this, _event: &MouseDownEvent, window, cx| {
                                                                                            this.open_table_ddl(
                                                                                                host_name_clone.clone(),
                                                                                                schema_clone.clone(),
                                                                                                table_clone.clone(),
                                                                                                window,
                                                                                                cx,
                                                                                            );
                                                                                        }
                                                                                    })
                                                                                )
                                                                            .child(
                                                                                h_flex()
                                                                                    .gap_2()
                                                                                    .items_center()
                                                                                    .child(
                                                                                        Icon::new(IconName::Archive)
                                                                                            .size(IconSize::Small),
                                                                                    )
                                                                                    .child(
                                                                                        Label::new(table)
                                                                                            .size(LabelSize::Small)
                                                                                            .color(Color::Muted),
                                                                                    ),
                                                                            )}))
                                                                    .into_any_element()
                                                            })
                                                    )
                                                })
                                        }))
                                )
                            })
                    })),
            )
            // Форма добавления
            .when(self.show_add_form, |this| {
                this.child(
                    v_flex()
                        .p_4()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().colors().border)
                        .gap_4()
                        .child(Label::new("Add new host").weight(FontWeight::MEDIUM))
                        // Host URL
                        .child(self.new_host_input.as_ref().unwrap().clone())
                        // Выбор типа авторизации
                        .child(
                            h_flex()
                                .gap_2()
                                .child(Button::new("none", "None").on_click(cx.listener(
                                    |this, _, window, cx| this.set_auth_type("none", window, cx),
                                )))
                                .child(Button::new("bearer", "Bearer").on_click(cx.listener(
                                    |this, _, window, cx| this.set_auth_type("bearer", window, cx),
                                )))
                                .child(Button::new("custom", "Custom Header").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.set_auth_type("custom-header", window, cx)
                                    }),
                                ))
                                .child(Button::new("basic", "Basic Auth").on_click(cx.listener(
                                    |this, _, window, cx| this.set_auth_type("basic", window, cx),
                                ))),
                        )
                        // Поля Bearer
                        .when(self.new_auth_type == "bearer", |this| {
                            this.child(
                                v_flex()
                                    .gap_2()
                                    .child(Label::new("Bearer Token").size(LabelSize::Small))
                                    .child(self.new_bearer_token_input.as_ref().unwrap().clone()),
                            )
                        })
                        // Поля Custom Header
                        .when(self.new_auth_type == "custom-header", |this| {
                            this.child(
                                v_flex()
                                    .gap_3()
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(Label::new("Header Name").size(LabelSize::Small))
                                            .child(
                                                self.new_header_name_input
                                                    .as_ref()
                                                    .unwrap()
                                                    .clone(),
                                            ),
                                    )
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(
                                                Label::new("Header Value").size(LabelSize::Small),
                                            )
                                            .child(
                                                self.new_header_value_input
                                                    .as_ref()
                                                    .unwrap()
                                                    .clone(),
                                            ),
                                    ),
                            )
                        })
                        // Поля Basic Auth
                        .when(self.new_auth_type == "basic", |this| {
                            this.child(
                                v_flex()
                                    .gap_3()
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(Label::new("Username").size(LabelSize::Small))
                                            .child(
                                                self.new_username_input.as_ref().unwrap().clone(),
                                            ),
                                    )
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(Label::new("Password").size(LabelSize::Small))
                                            .child(
                                                self.new_password_input.as_ref().unwrap().clone(),
                                            ),
                                    ),
                            )
                        })
                        // Кнопки
                        .child(
                            h_flex()
                                .gap_2()
                                .justify_end()
                                .child(Button::new("cancel", "Cancel").on_click(cx.listener(
                                    |this, _, window, cx| this.close_add_form(window, cx),
                                )))
                                .child(
                                    Button::new("save", "Save")
                                        .style(ButtonStyle::Filled)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.add_host(window, cx)
                                        })),
                                ),
                        ),
                )
            })
            .child(
                Button::new(
                    "add-host",
                    if self.show_add_form {
                        "✕ Close"
                    } else {
                        "＋ Add Host"
                    },
                )
                .style(ButtonStyle::Filled)
                .full_width()
                .on_click(cx.listener(|this, _, window, cx| this.toggle_add_form(window, cx))),
            )
    }
}

// Остальные трейты без изменений
impl Focusable for SqlApiExplorerPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for SqlApiExplorerPanel {}

impl Panel for SqlApiExplorerPanel {
    fn persistent_name() -> &'static str {
        "SqlApiExplorerPanel"
    }
    fn panel_key() -> &'static str {
        "SqlApiExplorerPanel"
    }
    fn position(&self, _w: &Window, _c: &App) -> DockPosition {
        DockPosition::Left
    }
    fn position_is_valid(&self, _p: DockPosition) -> bool {
        true
    }
    fn set_position(&mut self, _p: DockPosition, _w: &mut Window, _c: &mut Context<Self>) {}
    fn default_size(&self, _w: &Window, _c: &App) -> Pixels {
        px(400.)
    }
    fn icon(&self, _w: &Window, _c: &App) -> Option<IconName> {
        Some(IconName::DatabaseZap)
    }
    fn icon_tooltip(&self, _w: &Window, _c: &App) -> Option<&'static str> {
        Some("SQL API Explorer")
    }
    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleSqlApiExplorerPanel)
    }
    fn activation_priority(&self) -> u32 {
        750
    }
}

actions!(sql_api_explorer_panel, [ToggleSqlApiExplorerPanel]);

// ====================== SQL TABLE DATA VIEW ======================

use workspace::item::TabContentParams;
use gpui::AnyElement;
use gpui::SharedString;

#[derive(Clone)]
struct ColumnInfo {
    name: String,
    data_type: String,
}

pub struct SqlTableDataView {
    focus_handle: FocusHandle,
    host: HostConfig,
    schema: String,
    table: String,
    full_table_name: String,

    columns: Vec<ColumnInfo>,
    data: Vec<serde_json::Value>,
    total_rows: usize,

    limit: usize,
    offset: usize,
    where_clause: String,
    order_by: String,
    sort_column: Option<String>,
    sort_direction: String,

    loading: bool,
    status: String,

    // UI поля
    limit_input: Option<Entity<InputField>>,
    where_input: Option<Entity<InputField>>,

    order_input: Option<Entity<InputField>>,
    expanded_rows: HashSet<usize>,
}

impl SqlTableDataView {
    pub fn new(
        host: HostConfig,
        schema: String,
        table: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let full_table_name = if schema.is_empty() {
            table.clone()
        } else {
            format!("{}.{}", schema, table)
        };

        let mut this = Self {
            focus_handle: cx.focus_handle(),
            host,
            schema,
            table,
            full_table_name,
            columns: vec![],
            data: vec![],
            total_rows: 0,
            limit: 100,
            offset: 0,
            where_clause: String::new(),
            order_by: String::new(),
            sort_column: None,
            sort_direction: "ASC".to_string(),
            loading: true,
            status: "Loading...".to_string(),

            limit_input: None,
            where_input: None,
            order_input: None,
            expanded_rows: HashSet::new(),
        };

        this.load_data(cx);
        this
    }

    fn ensure_input_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.limit_input.is_none() {
            self.limit_input = Some(cx.new(|cx| InputField::new(window, cx, "100")));
        }
        if self.where_input.is_none() {
            self.where_input = Some(cx.new(|cx| InputField::new(window, cx, "")));
        }
        if self.order_input.is_none() {                                   // ← НОВОЕ
                self.order_input = Some(cx.new(|cx| InputField::new(window, cx, "")));
            }
    }

    fn toggle_row(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.expanded_rows.contains(&index) {
            self.expanded_rows.remove(&index);
        } else {
            self.expanded_rows.insert(index);
        }
        cx.notify();
    }

    fn load_data(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.status = "⏳ Loading data...".to_string();
        cx.notify();

        let host = self.host.clone();
        let schema = self.schema.clone();
        let table = self.table.clone();
        let limit = self.limit;
        let offset = self.offset;
        let where_clause = self.where_clause.clone();
        let order_by = self.order_by.clone();
        let sort_column = self.sort_column.clone();
        let sort_direction = self.sort_direction.clone();

        let client: Arc<dyn HttpClient> = cx.http_client();

        cx.spawn(async move |this, cx| {
            let result = Self::query_table_data_real(
                &host,
                &schema,
                &table,
                limit,
                offset,
                &where_clause,
                &order_by,
                sort_column.as_deref(),
                &sort_direction,
                client,
            )
            .await;

            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok((cols, data, total)) => {
                        view.columns = cols;
                        view.data = data;
                        view.total_rows = total;
                        view.status = format!("✅ Loaded {} rows out of {}", view.data.len(), view.total_rows);
                    }
                    Err(err) => {
                        view.status = format!("❌ {}", err);
                    }
                }
                view.loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    async fn query_table_data_real(
        host: &HostConfig,
        schema: &str,
        table: &str,
        limit: usize,
        offset: usize,
        where_clause: &str,
        order_by: &str,
        sort_column: Option<&str>,
        sort_direction: &str,
        client: Arc<dyn HttpClient>,
    ) -> Result<(Vec<ColumnInfo>, Vec<serde_json::Value>, usize), String> {
        let full_table = if schema.is_empty() {
            table.to_string()
        } else {
            format!("{}.{}", schema, table)
        };

        let cols_sql = format!(
            "SELECT column_name, data_type FROM information_schema.columns
             WHERE table_schema = '{}' AND table_name = '{}' ORDER BY ordinal_position",
            schema.replace("'", "''"), table.replace("'", "''")
        );

        let cols_result = Self::execute_sql_real(host, &cols_sql, client.clone()).await?;
        let mut columns = vec![];
        let mut select = "*".to_string();

        if let Some(data) = cols_result.get("data").and_then(|d| d.as_array()) {
            columns = data
                .iter()
                .filter_map(|row| {
                    let name = row.get("column_name")?.as_str()?.to_string();
                    let typ = row.get("data_type")?.as_str()?.to_string();
                    Some(ColumnInfo { name, data_type: typ })
                })
                .collect();
            if !columns.is_empty() {
                select = columns.iter().map(|c| format!("\"{}\"", c.name)).collect::<Vec<_>>().join(", ");
            }
        }

        let where_part = if where_clause.trim().is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clause)
        };

        let order_part = if !order_by.trim().is_empty() {
                format!("ORDER BY {}", order_by)
            } else if let Some(col) = sort_column {
                format!("ORDER BY \"{}\" {}", col, sort_direction)
            } else {
                String::new()
            };

        let sql = format!(
            "SELECT {} FROM {} {} {} LIMIT {} OFFSET {}",
            select, full_table, where_part, order_part, limit, offset
        );

        let result = Self::execute_sql_real(host, &sql, client.clone()).await?;
        let data = result
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();

        let count_sql = format!("SELECT COUNT(*) as cnt FROM {} {}", full_table, where_part);
        let count_res = Self::execute_sql_real(host, &count_sql, client).await?;
        let total = count_res
            .get("data")
            .and_then(|arr| arr.as_array())
            .and_then(|a| a.first())
            .and_then(|r| r.get("cnt"))
            .and_then(|v| v.as_i64())
            .map(|n| n as usize)
            .unwrap_or(data.len());

        Ok((columns, data, total))
    }

    async fn execute_sql_real(
        host: &HostConfig,
        sql: &str,
        client: Arc<dyn HttpClient>,
    ) -> Result<serde_json::Value, String> {
        let url = format!("{}/api/v1/dev/query-sql", host.name.trim_end_matches('/'));

        let mut builder = Request::builder()
            .method(Method::POST)
            .uri(url)
            .header("Content-Type", "application/json");

        if let Some(auth) = &host.auth {
            match auth.r#type.as_str() {
                "bearer" => if let Some(token) = &auth.token {
                    builder = builder.header("Authorization", format!("Bearer {}", token));
                },
                "custom-header" => if let (Some(name), Some(value)) = (&auth.header_name, &auth.header_value) {
                    builder = builder.header(name, value);
                },
                "basic" => if let (Some(username), Some(password)) = (&auth.username, &auth.password) {
                    let credentials = format!("{}:{}", username, password);
                    let encoded = BASE64_STANDARD.encode(credentials.as_bytes());
                    builder = builder.header("Authorization", format!("Basic {}", encoded));
                },
                _ => {}
            }
        }

        let body = json!({ "sql": sql }).to_string();

        let request = builder
            .body(AsyncBody::from(body.into_bytes()))
            .map_err(|e| format!("Failed to build request: {}", e))?;

        let response = client
            .send(request)
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        let mut body = response.into_body();
        let mut text = String::new();
        body.read_to_string(&mut text)
            .await
            .map_err(|e| format!("Failed to read body: {}", e))?;

        serde_json::from_str(&text).map_err(|e| format!("JSON parse error: {}", e))
    }

    fn apply_filters(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = &self.limit_input {
            if let Ok(l) = input.read(cx).text(cx).parse::<usize>() {
                self.limit = l.max(1);
            }
        }
        if let Some(input) = &self.where_input {
            self.where_clause = input.read(cx).text(cx).trim().to_string();
        }
        if let Some(input) = &self.order_input {                       // ← НОВОЕ
            self.order_by = input.read(cx).text(cx).trim().to_string();
        }

        self.offset = 0;
        self.load_data(cx);
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load_data(cx);
    }

    fn show_in_browser(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.data.is_empty() {
            self.status = "⚠️ No data available for export".to_string();
            cx.notify();
            return;
        }

        let columns = self.columns.clone();
        let data = self.data.clone();
        let table_name = self.full_table_name.clone();

        let colors = cx.theme().colors();
        let html_colors = HtmlThemeColors {
            background: hsla_to_hex(colors.background),
            editor_background: hsla_to_hex(colors.surface_background),
            border: hsla_to_hex(colors.border),
            text: hsla_to_hex(colors.text),
            accent: hsla_to_hex(colors.text_accent),
        };

        std::thread::spawn(move || {
            let html = generate_interactive_html_table(&table_name, &columns, &data, &html_colors);
            let temp_dir = std::env::temp_dir();
            let file_name = format!("sql_table_{}.html", Uuid::new_v4());
            let file_path = temp_dir.join(&file_name);

            if let Err(e) = std::fs::write(&file_path, &html) {
                eprintln!("Failed to write HTML file: {}", e);
                return;
            }

            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("cmd")
                    .args(["/C", "start", "", &file_path.to_string_lossy()])
                    .spawn()
                    .ok();
            }
            #[cfg(not(target_os = "windows"))]
            {
                std::process::Command::new("xdg-open")
                    .arg(&file_path)
                    .spawn()
                    .ok();
            }
        });

        self.status = "🌐 Opening in the browser...".to_string();
        cx.notify();
    }

    async fn fetch_ddl_real(
        host: &HostConfig,
        schema: &str,
        table: &str,
        client: Arc<dyn HttpClient>,
    ) -> Result<String, String> {
        let sql = format!(
            r#"
WITH cols AS (
    SELECT
        column_name,
        data_type,
        character_maximum_length,
        is_nullable,
        column_default,
        ordinal_position
    FROM information_schema.columns
    WHERE table_schema = '{}' AND table_name = '{}'
    ORDER BY ordinal_position
),
pk AS (
    SELECT string_agg(quote_ident(column_name), ', ') as pk_columns
    FROM information_schema.key_column_usage kcu
    JOIN information_schema.table_constraints tc
        ON kcu.constraint_name = tc.constraint_name
    WHERE tc.table_schema = '{}'
      AND tc.table_name = '{}'
      AND tc.constraint_type = 'PRIMARY KEY'
),
fk AS (
    SELECT
        tc.constraint_name,
        string_agg(DISTINCT quote_ident(kcu.column_name), ', ') as fk_columns,
        ccu.table_schema as ref_schema,
        ccu.table_name as ref_table,
        string_agg(DISTINCT quote_ident(ccu.column_name), ', ') as ref_columns,
        rc.update_rule,
        rc.delete_rule
    FROM information_schema.table_constraints tc
    JOIN information_schema.key_column_usage kcu
        ON tc.constraint_name = kcu.constraint_name
    JOIN information_schema.constraint_column_usage ccu
        ON tc.constraint_name = ccu.constraint_name
    JOIN information_schema.referential_constraints rc
        ON tc.constraint_name = rc.constraint_name
    WHERE tc.table_schema = '{}'
      AND tc.table_name = '{}'
      AND tc.constraint_type = 'FOREIGN KEY'
    GROUP BY tc.constraint_name, ccu.table_schema, ccu.table_name, rc.update_rule, rc.delete_rule
),
indexes AS (
    SELECT string_agg(indexdef || ';', E'\n') as index_statements
    FROM pg_indexes
    WHERE schemaname = '{}' AND tablename = '{}'
)
SELECT
    'CREATE TABLE ' || quote_ident('{}') || '.' || quote_ident('{}') || E'\n(\n' ||
    string_agg(
        '    ' || quote_ident(column_name) || ' ' ||
        CASE
            WHEN data_type = 'character varying' THEN 'varchar(' || COALESCE(character_maximum_length::text, '255') || ')'
            WHEN data_type = 'character' THEN 'char(' || character_maximum_length::text || ')'
            ELSE data_type
        END ||
        CASE WHEN is_nullable = 'NO' THEN ' NOT NULL' ELSE '' END ||
        COALESCE(' DEFAULT ' || column_default, ''),
        E',\n'
        ORDER BY ordinal_position
    ) || E'\n);' ||
    COALESCE(E'\n\n-- Primary Key:\nALTER TABLE ' || quote_ident('{}') || '.' || quote_ident('{}') ||
             ' ADD PRIMARY KEY (' || pk_columns || ');', '') ||
    COALESCE(
        (SELECT string_agg(
            E'\n-- Foreign Key:\nALTER TABLE ' || quote_ident('{}') || '.' || quote_ident('{}') ||
            ' ADD CONSTRAINT ' || quote_ident(constraint_name) ||
            ' FOREIGN KEY (' || fk_columns || ') REFERENCES ' ||
            quote_ident(ref_schema) || '.' || quote_ident(ref_table) ||
            ' (' || ref_columns || ')' ||
            CASE WHEN delete_rule != 'NO ACTION' THEN ' ON DELETE ' || delete_rule ELSE '' END ||
            CASE WHEN update_rule != 'NO ACTION' THEN ' ON UPDATE ' || update_rule ELSE '' END || ';',
            ''
        ) FROM fk),
        ''
    ) ||
    COALESCE(E'\n\n-- Indexes:\n' || index_statements, '') as full_ddl
FROM cols
LEFT JOIN pk ON true
LEFT JOIN indexes ON true
GROUP BY pk_columns, index_statements;
            "#,
            schema, table,          // cols
            schema, table,          // pk
            schema, table,          // fk
            schema, table,          // indexes
            schema, table,          // CREATE TABLE
            schema, table,          // PK
            schema, table           // FK
        );

        let result = Self::execute_sql_real(host, &sql, client).await?;

        let ddl = result
            .get("data")
            .and_then(|d| d.as_array())
            .and_then(|arr| arr.first())
            .and_then(|row| row.get("full_ddl"))
            .and_then(|v| v.as_str())
            .unwrap_or("Failed to generate DDL")
            .to_string();

        Ok(ddl)
    }

    // fn sort_by_column(&mut self, column: String, cx: &mut Context<Self>) {
    //     if self.sort_column.as_ref() == Some(&column) {
    //         self.sort_direction = if self.sort_direction == "ASC" { "DESC".to_string() } else { "ASC".to_string() };
    //     } else {
    //         self.sort_column = Some(column);
    //         self.sort_direction = "ASC".to_string();
    //     }
    //     self.offset = 0;
    //     self.load_data(cx);
    // }

    fn format_cell_value(&self, column_name: &str, value: &serde_json::Value) -> String {
        if value.is_null() {
            return "NULL".to_string();
        }
        if let Some(col) = self.columns.iter().find(|c| c.name == column_name) {
            if col.data_type == "bytea" {
                return "[bytea data]".to_string();
            }
        }
        match value {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Array(arr) => format!("[{} items]", arr.len()),
            serde_json::Value::Object(_) => "{...}".to_string(),
            _ => value.to_string(),
        }
    }

    fn render_rows_view(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_2()
            .children(self.data.iter().enumerate().map(|(i, row)| {
                let row_num = self.offset + i + 1;
                let is_expanded = self.expanded_rows.contains(&i);
                let row_obj = row.as_object();

                v_flex()
                    .child(
                        div()
                            .id(format!("row-{}", i))
                            .flex()
                            .justify_between()
                            .items_center()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .hover(|s| s.bg(cx.theme().colors().element_hover))
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.toggle_row(i, cx);
                            }))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        Icon::new(if is_expanded {
                                            IconName::ChevronDown
                                        } else {
                                            IconName::ChevronRight
                                        })
                                        .size(IconSize::Small),
                                    )
                                    .child(
                                        Label::new(format!("Row {}", row_num))
                                            .weight(FontWeight::MEDIUM),
                                    ),
                            )
                            .child(
                                Label::new(format!("({} fields)", self.columns.len()))
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                    )
                    .when(is_expanded, |this| {
                        this.child(
                            div()
                                .id(format!("row-detail-{}", i))
                                .pl_8()
                                .pr_3()
                                .py_2()
                                .w_full()
                                .bg(cx.theme().colors().element_background)
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().colors().border)
                                .child(
                                    v_flex()
                                        .gap_2()
                                        .children(self.columns.iter().map(|col| {
                                            let value = row_obj
                                                .and_then(|o| o.get(&col.name))
                                                .cloned()
                                                .unwrap_or(serde_json::Value::Null);
                                            let display = self.format_cell_value(&col.name, &value);

                                            h_flex()
                                                .gap_3()
                                                .items_start()
                                                .child(
                                                    div()
                                                        .w(px(180.))
                                                        .flex_shrink_0()
                                                        .child(
                                                            Label::new(&col.name)
                                                                .weight(FontWeight::MEDIUM)
                                                                .size(LabelSize::Small)
                                                                .color(Color::Muted),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .id(format!("value-{}-{}", i, col.name))
                                                        .flex_1()
                                                        .min_w_0()
                                                        .overflow_x_hidden()
                                                        .child(
                                                            Label::new(display.clone())
                                                                .size(LabelSize::Small)
                                                        )
                                                )
                                                .child(
                                                    Button::new(format!("copy-field-{}-{}", i, col.name), "📋")
                                                        .style(ButtonStyle::Subtle)
                                                        .on_click(move |_, _window, cx| {
                                                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(display.clone()));
                                                        })
                                                )
                                        }))
                                )
                        )
                    })
            }))
    }
}

impl Render for SqlTableDataView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_input_fields(window, cx);

        let limit_input = self.limit_input.as_ref().unwrap().clone();
        let where_input = self.where_input.as_ref().unwrap().clone();
        let order_input = self.order_input.as_ref().unwrap().clone();

        v_flex()
            .id("sql-table-data")
            .track_focus(&self.focus_handle)
            .size_full()
            .p_3()
            .gap_3()
            .bg(cx.theme().colors().panel_background)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(Label::new(format!("📋 {}", self.full_table_name)).size(LabelSize::Large))
                    .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("show_in_browser", "Show in browser")
                                            .style(ButtonStyle::Filled)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.show_in_browser(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("refresh", "Refresh")
                                            .style(ButtonStyle::Filled)
                                            .on_click(cx.listener(|this, _, _window, cx| this.refresh(cx))),
                                    ),
                            ),
            )
            .child(Label::new(&self.status).color(Color::Muted).size(LabelSize::Small))

            .child(
                h_flex()
                    .gap_4()
                    .items_end()
                    .child(
                        v_flex()
                            .flex_1()
                            .child(Label::new("Limit").size(LabelSize::Small))
                            .child(limit_input.clone()),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .child(Label::new("Where").size(LabelSize::Small))
                            .child(where_input.clone()),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .child(Label::new("Order by").size(LabelSize::Small))
                            .child(order_input.clone()),
                    )
                    .child(
                        Button::new("apply", "Apply")
                            .style(ButtonStyle::Filled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.apply_filters(window, cx);
                            })),
                    ),
            )

            // === Список строк с expand ===
            .child(
                div()
                    .id("sql-table-rows-scroll")
                    .flex_1()
                    .overflow_y_scroll()           // ← исправлено
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .rounded_md()
                    .child(
                        if self.loading {
                            Label::new("⏳ Loading...").into_any_element()
                        } else if self.data.is_empty() {
                            Label::new("No data").italic().into_any_element()
                        } else {
                            self.render_rows_view(window, cx).into_any_element()
                        },
                    ),
            )

            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        Button::new("prev", "← Previous")
                            .disabled(self.offset == 0)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                if this.offset >= this.limit {
                                    this.offset -= this.limit;
                                    this.load_data(cx);
                                }
                            })),
                    )
                    .child(
                        Label::new(format!(
                            "Rows {}-{} out of {}",
                            self.offset + 1,
                            (self.offset + self.limit).min(self.total_rows),
                            self.total_rows
                        ))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    )
                    .child(
                        Button::new("next", "Next →")
                            .disabled(self.offset + self.limit >= self.total_rows)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.offset += this.limit;
                                this.load_data(cx);
                            })),
                    ),
            )
    }
}

impl Focusable for SqlTableDataView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<()> for SqlTableDataView {}

impl workspace::Item for SqlTableDataView {
    type Event = ();

    fn tab_content(&self, _params: TabContentParams, _window: &Window, _cx: &App) -> AnyElement {
        Label::new(self.full_table_name.clone()).into_any_element()
    }

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        SharedString::from(self.full_table_name.clone())
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("sql_table_data_view")
    }
}

// ====================== ПРОСТОЙ DDL VIEW (без лишних зависимостей) ======================

pub struct DdlViewer {
    focus_handle: FocusHandle,
    title: String,
    ddl_text: String,
}

impl DdlViewer {
    pub fn new(title: String, ddl_text: String, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            title,
            ddl_text,
        }
    }

    pub fn update_ddl(&mut self, ddl_text: String, cx: &mut Context<Self>) {
            self.ddl_text = ddl_text;
            cx.notify();
        }
}

impl Render for DdlViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("ddl-viewer")
            .track_focus(&self.focus_handle)
            .size_full()
            .p_4()
            .bg(cx.theme().colors().panel_background)
            .child(
                // Заголовок + кнопка Копировать
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        Label::new(&self.title)
                            .size(LabelSize::Large)
                            .weight(FontWeight::MEDIUM),
                    )
                    .child(
                        Button::new("copy-ddl", "📋 Copy")
                            .style(ButtonStyle::Filled)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                // Копируем текст в буфер обмена
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(this.ddl_text.clone()));
                            })),
                    ),
            )
            .child(
                div()
                    .id("ddl-content")
                    .flex_1()
                    .overflow_y_scroll()
                    .overflow_x_scroll()
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .rounded_md()
                    .p_4()
                    .bg(cx.theme().colors().editor_background)
                    .font_family("monospace")
                    .cursor_text()
                    .child(
                        Label::new(&self.ddl_text)
                            .size(LabelSize::Small)
                    )
            )
    }
}

impl Focusable for DdlViewer {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<()> for DdlViewer {}

impl workspace::Item for DdlViewer {
    type Event = ();

    fn tab_content(&self, _params: TabContentParams, _window: &Window, _cx: &App) -> AnyElement {
        Label::new(self.title.clone()).into_any_element()
    }

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        SharedString::from(self.title.clone())
    }
}

pub fn init(cx: &mut App) {
    println!("🚀 SqlApiExplorerPanel — full authorization form is ready!");

    if !cx.has_global::<SqlApiHosts>() {
        cx.set_global(SqlApiHosts { hosts: vec![] });
    }

    cx.observe_new(|workspace: &mut Workspace, mut window, cx| {
        workspace.register_action(
            |workspace: &mut Workspace, _action: &ToggleSqlApiExplorerPanel, window, cx| {
                workspace.toggle_panel_focus::<SqlApiExplorerPanel>(window, cx);
            },
        );

        let workspace_entity = cx.entity();
        let panel = cx.new(|cx| SqlApiExplorerPanel::new(cx, workspace_entity.clone()));
        workspace.add_panel(panel, window.as_mut().unwrap(), cx);
    })
    .detach();
}
