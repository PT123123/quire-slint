// MCP 工具面：Agent 能对这份笔记库做什么，规定在这里。
//
// 纪律（与用户约定一致）：
// - 工具面只有整理器的笔记与任务。页面不在协议里——没有页面读、没有页面
//   写、更没有页面删；Agent 的活动范围就是笔记与任务这两张清单。
// - 删除只有「移到回收站」一个动词：`org_trash_note` / `org_trash_task`，
//   打 `deleted_at` 戳的 Update（core 的回收站语义，SPEC §四十一），可用
//   `restore_*` 恢复。硬删（`org_delete_note`，UI 自己按钮用的那个）不在
//   Agent 的菜单上。
// - 其余添加、阅读、修改自由开放，走的都是应用自己的写入路径
//   （`AppState::org_*` → `exec_org` → record）：与键盘输入同一条漏斗，
//   Agent 的改动照样进撤销栈。
//
// 线程模型：HTTP 线程把 `tools/call` 翻译成一个 `McpJob` 丢进 channel；
// `arm_pump` 装的定时器在 UI 线程排水，执行后原路回复，并把每一次调用写进
// MCP 日志、发布到设置里的 MCP 页。AppState 是 Rc，这是唯一能让 Agent 触碰
// 它的通道——与 sync 引擎的 Job 泵同一个模式。

use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use slint::ComponentHandle;

use crate::app::controller;
use crate::app::state::AppState;
use crate::mcp::ToolDef;
use crate::UIState;

/// One stored line of the MCP log (settings row "mcp.log"). The sync engine's
/// `LogLine` shape with the tool name where the peer goes — both logs are a
/// stamped tail one settings row carries, and both are dated by an age label
/// computed at publish time, not by an absolute clock.
#[derive(Serialize, Deserialize)]
pub struct McpLogLine {
    pub at: String,
    pub tool: String,
    pub ok: bool,
    pub message: String,
}

/// 回复通道。UI 线程执行完把结果（或错误文案）发回去。
pub enum McpJob {
    Call {
        name: String,
        args: Value,
        reply: Sender<Result<Value, String>>,
    },
}

/// 一条 HTTP `tools/call` 的发送端——main.rs 把它包进 `mcp::CallFn`。
pub fn job_sender(tx: Sender<McpJob>) -> crate::mcp::CallFn {
    Arc::new(move |name: &str, args: Value| {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        tx.send(McpJob::Call {
            name: name.to_string(),
            args,
            reply: reply_tx,
        })
        .map_err(|_| "MCP 泵已停止（应用正在退出）".to_string())?;
        reply_rx
            .recv_timeout(Duration::from_secs(120))
            .map_err(|_| "MCP 调用超时".to_string())?
    })
}

/// UI 线程的泵：250ms 排一次 job 队列（与 sync 泵同一个纪律——漏掉的
/// Timer 会被 cancel，所以 Box::leak）。执行工具 → 记日志 → 发布 → 回复。
pub fn arm_pump(ui: &crate::AppWindow, state: &Rc<AppState>, rx: Receiver<McpJob>) {
    let ui_w = ui.as_weak();
    let s = state.clone();
    let t: &'static slint::Timer = Box::leak(Box::new(slint::Timer::default()));
    t.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(250),
        move || {
            let Some(ui) = ui_w.upgrade() else { return };
            let g = ui.global::<UIState>();
            while let Ok(job) = rx.try_recv() {
                let McpJob::Call { name, args, reply } = job;
                let result = exec(&g, &s, &name, &args);
                // 每一次调用都留痕：工具、成败、一句人话（MCP 页要读的是
                // 这个，不是 JSON）。sync 的 log_push 同一个动作，50 条尾巴。
                s.mcp_log_push(&name, result.is_ok(), &log_message(&args, &result));
                publish_log(&g, &s);
                let _ = reply.send(result);
            }
        },
    );
    // 装泵时先发布一次：设置里 mcp.log 存着的旧日志要在页面上等得到。
    publish_log(&ui.global::<UIState>(), state);
}

// ─── 工具定义 ────────────────────────────────────────────────────────────────

fn schema(props: Value, required: &[&str]) -> Value {
    let mut v = json!({ "type": "object", "properties": props });
    if !required.is_empty() {
        v["required"] = json!(required);
    }
    v
}

/// 全部工具。名字用英文（Agent 的调用面），描述里写清中文语境。
pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "list_notes",
            description: "列出整理器的笔记（含在回收站里的，用 include_trashed 控制）。",
            input_schema: schema(
                json!({ "include_trashed": { "type": "boolean", "description": "默认 false，只列未删除的" } }),
                &[],
            ),
        },
        ToolDef {
            name: "create_note",
            description: "新建一条笔记。tags 用逗号分隔。",
            input_schema: schema(
                json!({
                    "title": { "type": "string" },
                    "body": { "type": "string" },
                    "tags": { "type": "string", "description": "逗号分隔，如 \"工作,项目\"" }
                }),
                &["title"],
            ),
        },
        ToolDef {
            name: "update_note",
            description: "修改一条笔记的标题/正文/标签；只改给出的字段。",
            input_schema: schema(
                json!({
                    "note_id": { "type": "integer" },
                    "title": { "type": "string" },
                    "body": { "type": "string" },
                    "tags": { "type": "string" }
                }),
                &["note_id"],
            ),
        },
        ToolDef {
            name: "trash_note",
            description: "把一条笔记移到回收站（可恢复；不彻底删除）。",
            input_schema: schema(
                json!({ "note_id": { "type": "integer" } }),
                &["note_id"],
            ),
        },
        ToolDef {
            name: "restore_note",
            description: "把一条笔记从回收站恢复。",
            input_schema: schema(
                json!({ "note_id": { "type": "integer" } }),
                &["note_id"],
            ),
        },
        ToolDef {
            name: "list_tasks",
            description: "列出整理器的任务（含回收站的，用 include_trashed 控制）。返回清单 id、标题、备注、截止日、优先级、完成状态。",
            input_schema: schema(
                json!({ "include_trashed": { "type": "boolean" } }),
                &[],
            ),
        },
        ToolDef {
            name: "create_task",
            description: "新建一个任务（默认进收集箱）。priority ∈ none/low/medium/high，due 是 YYYY-MM-DD，tags 逗号分隔。",
            input_schema: schema(
                json!({
                    "title": { "type": "string" },
                    "notes": { "type": "string" },
                    "due": { "type": "string" },
                    "priority": { "type": "string", "enum": ["none", "low", "medium", "high"] },
                    "tags": { "type": "string" },
                    "list_id": { "type": "integer", "description": "清单 id；缺省为收集箱" }
                }),
                &["title"],
            ),
        },
        ToolDef {
            name: "update_task",
            description: "修改一个任务的标题/备注/截止日/优先级/完成状态/标签；只改给出的字段。",
            input_schema: schema(
                json!({
                    "task_id": { "type": "integer" },
                    "title": { "type": "string" },
                    "notes": { "type": "string" },
                    "due": { "type": "string" },
                    "priority": { "type": "string", "enum": ["none", "low", "medium", "high"] },
                    "done": { "type": "boolean" },
                    "tags": { "type": "string" }
                }),
                &["task_id"],
            ),
        },
        ToolDef {
            name: "trash_task",
            description: "把一个任务移到回收站（可恢复；不彻底删除）。",
            input_schema: schema(
                json!({ "task_id": { "type": "integer" } }),
                &["task_id"],
            ),
        },
        ToolDef {
            name: "restore_task",
            description: "把一个任务从回收站恢复。",
            input_schema: schema(
                json!({ "task_id": { "type": "integer" } }),
                &["task_id"],
            ),
        },
    ]
}

/// 「给 Agent 的提示词」：用户贴进 AI 对话的一段话，说明 Quire 的接口能做什么、
/// 有哪些工具、参数怎么填、守什么约定。工具清单和参数都从 tool_defs() 生成 —
/// 接口加了新工具或改了参数，这段话不会和实际能力脱节。刻意不含令牌：这段文字
/// 会被贴到远端助手里，连接配置（含令牌）留在页面上另一处复制。
pub fn agent_prompt(port: u16) -> String {
    let mut s = String::from(
        "Quire 是运行在这台电脑上的笔记与任务应用，通过 MCP 接口开放下面这些工具。\n\
         需要记录、整理或修改笔记与任务时，请直接调用这些工具，不要改动 Quire 的数据文件。\n\n\
         可用工具：\n",
    );
    for t in tool_defs() {
        s.push_str(&format!("- {}：{}\n", t.name, t.description));
        // 参数行从工具自己的 input_schema 拆：必填在前，可选项带类型和枚举值。
        let empty = serde_json::Value::Null;
        let props = t.input_schema.get("properties").unwrap_or(&empty);
        let required: Vec<&str> = t
            .input_schema
            .get("required")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut parts: Vec<String> = Vec::new();
        for key in &required {
            if let Some(p) = props.get(key) {
                let ty = p.get("enum").map(|_| "枚举").unwrap_or_else(|| {
                    p.get("type").and_then(Value::as_str).unwrap_or("值")
                });
                parts.push(format!("{key}({ty})"));
            }
        }
        if let Some(obj) = props.as_object() {
            let optional: Vec<String> = obj
                .iter()
                .filter(|(k, _)| !required.contains(&k.as_str()))
                .map(|(k, p)| {
                    let mut d = format!("{k}({})", p.get("enum").map(|_| "枚举").unwrap_or_else(|| {
                        p.get("type").and_then(Value::as_str).unwrap_or("值")
                    }));
                    if let Some(ev) = p.get("enum").and_then(Value::as_array) {
                        let vals: Vec<&str> = ev.iter().filter_map(Value::as_str).collect();
                        d.push_str(&format!("：{}", vals.join("|")));
                    }
                    d
                })
                .collect();
            if !optional.is_empty() {
                parts.push(format!("可选 {}", optional.join("、")));
            }
        }
        if !parts.is_empty() {
            s.push_str(&format!("  参数：{}\n", parts.join("；")));
        }
    }
    s.push_str(&format!(
        "\n使用约定：\n\
         - 修改或删除前，先用 list_notes / list_tasks 查到对应的 note_id / task_id。\n\
         - trash_* 只把条目移入回收站，可用 restore_* 恢复；接口不提供彻底删除。\n\
         - tags 用逗号分隔多个标签，支持「项目/子标签」这样的层级写法。\n\
         - update_* 只修改给出的字段，没给出的字段保持原样。\n\
         - HTTP 服务在 http://127.0.0.1:{port}；stdio 与 HTTP 的连接配置（含令牌）在 Quire 设置 → MCP 页复制。\n\
         - 客户端不走 MCP 配置时，也可以按 JSON-RPC 2.0 直接调：每次 POST 一条消息，\n\
         \x20 请求形如 {{\"method\":\"tools/call\",\"params\":{{\"name\":\"create_note\",\"arguments\":{{\"title\":\"示例\"}}}}}}。\n"
    ));
    s
}

// ─── 参数与执行 ──────────────────────────────────────────────────────────────

fn arg_i64(args: &Value, key: &str) -> Result<i64, String> {
    args.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("缺少或非法参数：{key}"))
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("缺少或非法参数：{key}"))
}

fn arg_opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn arg_opt_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

fn priority_slot(name: &str) -> Result<i32, String> {
    match name.trim().to_ascii_lowercase().as_str() {
        "none" => Ok(0),
        "low" => Ok(1),
        "medium" => Ok(2),
        "high" => Ok(3),
        other => Err(format!("未知优先级：{other}（none/low/medium/high）")),
    }
}

/// 一个工具调用。入参是 JSON；返回值是给 Agent 的 JSON。所有文案中文，
/// 因为错误会原样进对话。
pub fn exec(g: &UIState<'_>, state: &Rc<AppState>, name: &str, args: &Value) -> Result<Value, String> {
    let result = match name {
        "list_notes" => list_notes(state, arg_opt_bool(args, "include_trashed").unwrap_or(false)),
        "create_note" => create_note(
            state,
            arg_str(args, "title")?,
            arg_opt_str(args, "body").unwrap_or(""),
            arg_opt_str(args, "tags").unwrap_or(""),
        ),
        "update_note" => update_note(state, arg_i64(args, "note_id")?, args),
        "trash_note" => trash_note(state, arg_i64(args, "note_id")?),
        "restore_note" => restore_note(state, arg_i64(args, "note_id")?),
        "list_tasks" => list_tasks(state, arg_opt_bool(args, "include_trashed").unwrap_or(false)),
        "create_task" => create_task(state, args),
        "update_task" => update_task(state, arg_i64(args, "task_id")?, args),
        "trash_task" => trash_task(state, arg_i64(args, "task_id")?),
        "restore_task" => restore_task(state, arg_i64(args, "task_id")?),
        other => return Err(format!("未知工具：{other}")),
    };
    // 写操作统一在这里收尾：整理器只有一行刷新——org_refresh（今日计数 +
    // 重建整理器模型）。页面那套标题/侧栏/投影的刷新用不上了，工具面没有
    // 页面写；读操作不动状态，什么都不刷。
    if WRITES.contains(&name) {
        controller::org_refresh(g, state);
    }
    result
}

/// 会改动库的工具集合。刷新跟着这张表走，与 match 臂一一对应。
const WRITES: [&str; 8] = [
    "create_note",
    "update_note",
    "trash_note",
    "restore_note",
    "create_task",
    "update_task",
    "trash_task",
    "restore_task",
];

// ─── 整理器：笔记 ────────────────────────────────────────────────────────────

fn note_row(n: &crate::core::organizer::Note) -> Value {
    json!({
        "id": n.id.0,
        "title": n.title,
        "body": n.body,
        "tags": n.tags,
        "pinned": n.pinned,
        "trashed": n.is_trashed(),
        "edited": n.edited,
    })
}

fn list_notes(state: &Rc<AppState>, include_trashed: bool) -> Result<Value, String> {
    let cat = state.org_catalog();
    let rows: Vec<Value> = cat
        .notes
        .iter()
        .filter(|n| include_trashed || !n.is_trashed())
        .map(note_row)
        .collect();
    Ok(json!({ "notes": rows }))
}

fn create_note(state: &Rc<AppState>, title: &str, body: &str, tags: &str) -> Result<Value, String> {
    let id = state.org_create_note().ok_or("笔记创建失败。")?;
    let _ = state.org_note_title(id, title.to_string());
    if !body.is_empty() {
        let _ = state.org_note_body(id, body.to_string());
    }
    if !tags.trim().is_empty() {
        let _ = state.org_note_tags(id, tags.to_string());
    }
    Ok(json!({ "id": id }))
}

fn update_note(state: &Rc<AppState>, id: i64, args: &Value) -> Result<Value, String> {
    if state.org_catalog().note(crate::core::organizer::NoteId(id.max(0) as u64)).is_none() {
        return Err(format!("笔记 {id} 不存在。"));
    }
    if let Some(t) = arg_opt_str(args, "title") {
        state.org_note_title(id, t.to_string());
    }
    if let Some(b) = arg_opt_str(args, "body") {
        state.org_note_body(id, b.to_string());
    }
    if let Some(tg) = arg_opt_str(args, "tags") {
        state.org_note_tags(id, tg.to_string());
    }
    Ok(json!({ "ok": true }))
}

/// 回收站，不是删除：`org_trash_note` 是打 `deleted_at` 戳的 Update，
/// `org_delete_note`（硬删）根本不在这个 match 里。
fn trash_note(state: &Rc<AppState>, id: i64) -> Result<Value, String> {
    match state.org_trash_note(id) {
        Some(_) => Ok(json!({ "ok": true })),
        None => Err(format!("笔记 {id} 不存在（或已在回收站）。")),
    }
}

fn restore_note(state: &Rc<AppState>, id: i64) -> Result<Value, String> {
    match state.org_restore_note(id) {
        Some(_) => Ok(json!({ "ok": true })),
        None => Err(format!("笔记 {id} 不在回收站。")),
    }
}

// ─── 整理器：任务 ────────────────────────────────────────────────────────────

fn task_row(t: &crate::core::organizer::Task) -> Value {
    json!({
        "id": t.id.0,
        "list": t.list.0,
        "title": t.title,
        "notes": t.notes,
        "due": t.due,
        "priority": t.priority.as_str(),
        "done": t.done,
        "tags": t.tags,
        "subtasks": t.subtasks.iter().map(|s| json!({"id": s.id, "title": s.title, "done": s.done})).collect::<Vec<_>>(),
        "trashed": t.is_trashed(),
    })
}

fn list_tasks(state: &Rc<AppState>, include_trashed: bool) -> Result<Value, String> {
    let cat = state.org_catalog();
    let rows: Vec<Value> = cat
        .tasks
        .iter()
        .filter(|t| include_trashed || !t.is_trashed())
        .map(task_row)
        .collect();
    Ok(json!({ "tasks": rows }))
}

fn create_task(state: &Rc<AppState>, args: &Value) -> Result<Value, String> {
    let list = args.get("list_id").and_then(Value::as_i64).unwrap_or(0);
    let tags: Vec<String> = arg_opt_str(args, "tags")
        .map(|tg| {
            tg.split(|c| c == ',' || c == '，')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let id = state
        .org_create_task_tagged(list, tags)
        .ok_or("任务创建失败。")?;
    let title = arg_str(args, "title")?;
    state.org_task_title(id, title.to_string());
    if let Some(n) = arg_opt_str(args, "notes") {
        if !n.is_empty() {
            state.org_task_notes(id, n.to_string());
        }
    }
    if let Some(d) = arg_opt_str(args, "due") {
        if !d.trim().is_empty() {
            state.org_task_due(id, d.trim().to_string());
        }
    }
    if let Some(p) = arg_opt_str(args, "priority") {
        state.org_task_priority(id, priority_slot(p)?);
    }
    Ok(json!({ "id": id }))
}

fn update_task(state: &Rc<AppState>, id: i64, args: &Value) -> Result<Value, String> {
    if !state.org_catalog().tasks.iter().any(|t| t.id.0 == id.max(0) as u64) {
        return Err(format!("任务 {id} 不存在。"));
    }
    if let Some(t) = arg_opt_str(args, "title") {
        state.org_task_title(id, t.to_string());
    }
    if let Some(n) = arg_opt_str(args, "notes") {
        state.org_task_notes(id, n.to_string());
    }
    if let Some(d) = arg_opt_str(args, "due") {
        state.org_task_due(id, d.trim().to_string());
    }
    if let Some(p) = arg_opt_str(args, "priority") {
        state.org_task_priority(id, priority_slot(p)?);
    }
    if let Some(done) = arg_opt_bool(args, "done") {
        state.org_task_done(id, done);
    }
    if let Some(tg) = arg_opt_str(args, "tags") {
        state.org_task_tags(id, tg.to_string());
    }
    Ok(json!({ "ok": true }))
}

/// 与 trash_note 同一条纪律：进回收站，不硬删。
fn trash_task(state: &Rc<AppState>, id: i64) -> Result<Value, String> {
    match state.org_trash_task(id) {
        Some(_) => Ok(json!({ "ok": true })),
        None => Err(format!("任务 {id} 不存在（或已在回收站）。")),
    }
}

fn restore_task(state: &Rc<AppState>, id: i64) -> Result<Value, String> {
    match state.org_restore_task(id) {
        Some(_) => Ok(json!({ "ok": true })),
        None => Err(format!("任务 {id} 不在回收站。")),
    }
}

// ─── MCP 日志 ────────────────────────────────────────────────────────────────

/// 日志的一句话：谁（标题或 #id）+ 怎么样（建成的 id / 完成 / 失败文案）。
/// MCP 页的日志给人读，所以是人话，不是回显的 JSON。
fn log_message(args: &Value, result: &Result<Value, String>) -> String {
    let subject = args
        .get("title")
        .and_then(Value::as_str)
        .map(|t| format!("“{t}”"))
        .or_else(|| {
            ["note_id", "task_id"].iter().find_map(|k| {
                args.get(*k).and_then(Value::as_i64).map(|v| format!("#{v}"))
            })
        })
        .unwrap_or_default();
    let tail = match result {
        Ok(v) => match v.get("id").and_then(Value::as_i64) {
            Some(i) => format!("已创建为 #{i}"),
            None => "完成".to_string(),
        },
        Err(e) => format!("失败：{e}"),
    };
    if subject.is_empty() {
        tail
    } else {
        format!("{subject} {tail}")
    }
}

/// 把存储的日志发布到 MCP 页。年龄在发布这一刻算（sync 的 refresh_sync_ui
/// 同一个规则），所以旧行会随每次发布重新变旧，而不是停在入栈那一刻。
pub fn publish_log(g: &UIState<'_>, state: &AppState) {
    let now = crate::services::sync::engine::now_unix();
    let rows: Vec<crate::McpLogRow> = state
        .mcp_log()
        .iter()
        .rev()
        .map(|l| crate::McpLogRow {
            when: controller::sync_age_label(&l.at, now).into(),
            tool: l.tool.clone().into(),
            ok: l.ok,
            message: l.message.clone().into(),
        })
        .collect();
    g.set_mcp_log(Rc::new(slint::VecModel::from(rows)).into());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 提示词的价值在于它是从 tool_defs() 生成的：接口加了新工具，这段文字
    /// 不会漏它；工具的中文描述原样进清单，Agent 读到的就是服务端答的。
    #[test]
    fn agent_prompt_names_every_tool_with_its_own_description() {
        let p = agent_prompt(5880);
        for t in tool_defs() {
            assert!(p.contains(t.name), "提示词少了工具 {}", t.name);
            assert!(p.contains(t.description), "提示词没带上 {} 的描述", t.name);
        }
        assert!(p.contains("http://127.0.0.1:5880"), "HTTP 地址要在");
        // 参数行也从 schema 生成：必填名和枚举值都得看得见，JSON-RPC 直调的
        // 例子要在 — 不配 MCP 的 Agent 照着它也能调。
        assert!(p.contains("note_id(integer)"), "必填参数要带类型");
        assert!(p.contains("none|low|medium|high"), "枚举值要展开");
        assert!(p.contains("tools/call"), "JSON-RPC 直调例子要在");
    }

    /// 提示词会被整段贴进（可能是远端的）对话，鉴权细节不能在里面 — 「令牌」
    /// 这个词可以出现（它只是告诉 Agent 去哪拿），Bearer 头不行，因为那意味着
    /// 有令牌被拼进了文案。
    #[test]
    fn agent_prompt_carries_no_credential() {
        let p = agent_prompt(5880);
        assert!(!p.contains("Bearer"), "提示词不该出现鉴权头");
    }
}
