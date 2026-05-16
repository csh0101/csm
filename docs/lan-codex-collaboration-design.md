# 局域网 Codex 会话协作感知设计稿

## 1. 背景

当前 Codex Session Manager 已能扫描、预览、标注和总结本机 Codex session JSONL。下一阶段可以把它从“个人会话管理器”扩展为“局域网协作感知工具”：当多名开发者在同一项目或相邻模块上使用 Codex 时，客户端之间可以在局域网内发现彼此，订阅彼此的 session JSONL 变化，并用本地 `codex exec --sandbox read-only` 对增量内容做旁路分析。

这个能力不替代 Git、PR、Issue、即时通讯或 Codex 本身，而是在开发过程中提供低侵入的协作提示，例如：

- 开发边界是否重合。
- 对方正在修改的功能是否会影响自己。
- 对方的实现思路和自己的计划是否冲突。
- 多人协作时是否需要同步接口、数据模型、配置或测试策略。
- 某个 session 的结论是否需要其他成员确认。

## 2. 产品定位

该功能可以命名为 **LAN Collaboration Awareness**，中文可称为“局域网协作感知”。

核心定位：

> 在可信局域网内，以 Codex session JSONL 增量为信号源，生成实时、旁路、非阻塞的协作提示。

它不是中心化协作平台，也不是远程代码同步工具。它只读取用户显式授权的 session 摘要、项目路径和必要元数据，并把分析结果展示为本地提示。

## 3. 目标用户

| 用户角色 | 核心诉求 | 典型场景 |
| --- | --- | --- |
| 双人/小团队开发者 | 知道对方 Codex 正在做什么，避免重复和冲突 | 两人同时改同一个 backend 模块 |
| Tech Lead | 观察多个成员的工作边界和风险 | 多人并行推进同一个项目的 API、前端、测试 |
| Pair Programming 用户 | 让 Codex session 之间互相校验 | A 负责实现，B 负责验证和边界检查 |
| 本地工具爱好者 | 不想上云，但想要局域网协作 | 同办公室、同 VPN、同 Wi-Fi 网络 |

## 4. 非目标范围

1. 不做云端账号体系。
2. 不做公网穿透。
3. 不同步源码文件。
4. 不直接修改任何人的 session JSONL。
5. 不直接把对方 session 注入自己的 Codex 主会话。
6. 不在 MVP 中实现复杂权限模型、审计中心或企业级合规。
7. 不承诺分析结果一定正确，所有提示必须标明“旁路建议”。

## 5. 核心概念

### 5.1 Peer

Peer 表示局域网内一个运行中的 Codex Session Manager 客户端。

字段建议：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| peerId | string | 本机稳定 ID，首次启动生成 |
| displayName | string | 用户可编辑名称 |
| hostname | string | 机器名 |
| appVersion | string | 客户端版本 |
| publicKey | string | 用于认证和加密 |
| advertisedProjects | ProjectPresence[] | 对外声明的项目活动 |
| lastSeenAt | ISO string | 最近发现时间 |

### 5.2 Project Presence

表示某个 peer 正在某个项目上活跃。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| projectId | string | 项目标识，建议由 repo root + remote URL hash 生成 |
| projectPathLabel | string | 可展示路径名，例如 `dev-machine` |
| gitRemote | string/null | 可选，经过脱敏或 hash 后广播 |
| gitBranch | string/null | 当前分支，可由用户选择是否公开 |
| workingSince | ISO string | 本轮活跃起点 |
| latestSessionAt | ISO string | 最新 session 记录时间 |

### 5.3 Subscription

订阅关系表示“我愿意接收某个 peer 在某个项目上的 session 增量”。

订阅粒度建议：

1. `peer + project`：订阅某个人某个项目。
2. `topic`：订阅某类提示，例如 API 风险、测试风险、冲突风险。

### 5.4 Share Policy

Share Policy 定义“哪些 session 可以暴露给订阅者”。项目白名单只解决“哪个项目可分享”，label 规则解决“项目内哪些 session 可分享”。

建议把 label 作为 MVP 的核心分享条件，而不是后续增强能力。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| allowedProjectIds | string[] | 可分享项目 |
| requiredLabels | string[] | session 必须包含任一 label 才可分享 |
| blockedLabels | string[] | 命中任一 label 时禁止分享 |
| defaultShareMode | none/labeled/all | 默认分享模式，建议默认 `labeled` |
| shareFullFinalAnswer | boolean | 是否允许分享完整最终回复 |
| maxDeltaChars | number | 单条 delta 最大外发字符数 |
| requirePreviewBeforeShare | boolean | 首次分享前是否展示预览 |

推荐内置 label 语义：

| Label | 语义 |
| --- | --- |
| `share` | 允许对订阅者分享该 session |
| `team` | 可对可信协作者分享 |
| `private` | 禁止分享 |
| `secret` | 禁止分享，并提示用户检查敏感信息 |
| `review` | 可作为互相确认/评审材料 |

默认策略建议：

```json
{
  "defaultShareMode": "labeled",
  "requiredLabels": ["share", "team", "review"],
  "blockedLabels": ["private", "secret"],
  "shareFullFinalAnswer": false,
  "maxDeltaChars": 1200,
  "requirePreviewBeforeShare": true
}
```

### 5.5 Session Delta

Session Delta 是从对方 JSONL 中抽取的增量事件，不是完整 JSONL 文件。

建议只同步结构化摘要：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| deltaId | string | 去重 ID |
| peerId | string | 来源 peer |
| projectId | string | 项目 |
| sessionId | string | Codex 原生 session id |
| timestamp | ISO string | JSONL 记录时间 |
| role | user/assistant/tool | 记录来源 |
| eventType | message/toolOutput/final/commit/test/error | 归一化类型 |
| textExcerpt | string | 限长文本 |
| pathsMentioned | string[] | 提到的文件路径 |
| commandsMentioned | string[] | 提到的命令 |
| gitRefs | string[] | 提到的 commit/branch/PR |
| sensitivity | normal/private/secretSuspected | 本地脱敏判断 |

## 6. 用户故事

### 6.1 发现附近协作者

1. 用户打开 app。
2. app 通过 mDNS/Bonjour 广播自己的存在。
3. 侧边栏显示“附近协作者”。
4. 用户看到同一局域网内的 peer 列表。
5. 用户点击某个 peer，查看其公开项目 presence。

### 6.2 订阅同一项目

1. 用户发现 Alice 正在 `dev-machine` 项目上工作。
2. 用户点击“订阅该项目”。
3. Alice 端收到订阅请求，选择允许。
4. Bob 端先触发一次首次订阅基线分析。
5. Bob 本机启动 `codex exec --sandbox read-only`，由 Codex Worker 通过受控 `curl` 访问 Alice 的只读协作接口，拉取该项目已授权、已过滤、已脱敏、已限长的 session 列表、详情摘要和必要 delta。
6. Codex Worker 把 Alice 的可分享 session 内容和 Bob 本地 session 放在同一个时间窗口内分析，生成一份 Markdown 协作总结。
7. 基线分析完成后，Bob 端再持续接收 Alice 的 session delta，用增量分析更新协作总结。
8. Bob 看到旁路提示：“你们都在触碰 gitops template，存在调度策略重叠风险”。

默认情况下，Alice 只会分享带有 `share`、`team` 或 `review` label 的 session。未打分享 label 的 session 只参与 Alice 本地分析，不会外发。

### 6.3 互相确认

1. Alice 的 session 得出一个结论：“CPU/GPU node selector 应由 SKU 推导”。
2. Bob 的 app 分析后生成确认请求：“该结论影响你的 scheduler 配置，是否确认？”
3. Bob 点击“确认/有疑问/忽略”。
4. Alice 端看到轻量反馈。

## 7. 系统架构

```text
Codex Session Manager.app
  ├─ React UI
  ├─ Tauri commands
  ├─ Local Session Scanner
  ├─ LAN Discovery Service
  ├─ Peer Subscription Service
  ├─ Session Delta Extractor
  ├─ Local Analysis Worker
  └─ Collaboration Hint Store

LAN
  ├─ mDNS/Bonjour discovery
  └─ peer-to-peer HTTPS/WebSocket
```

推荐仍保持本地优先：

- 每台机器保存自己的 metadata。
- 每台机器自行决定分享范围。
- 每台机器本地运行分析。
- 不需要中心服务器即可工作。

## 8. 技术方案

### 8.1 局域网发现

MVP 推荐使用 mDNS/Bonjour。

广播服务名：

```text
_csm-codex._tcp.local
```

广播内容：

```json
{
  "peerId": "peer_abc",
  "displayName": "Alice MacBook",
  "version": "0.2.0",
  "port": 45678,
  "publicKeyFingerprint": "sha256:..."
}
```

发现后不立即订阅，只显示 presence。订阅必须用户确认。

### 8.2 连接协议

MVP 可使用本地 HTTP + WebSocket：

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/peer/health` | peer 健康检查 |
| GET | `/peer/projects` | 获取对方公开项目 presence |
| GET | `/peer/sessions` | 获取对方已授权分享的 session 列表/摘要 |
| GET | `/peer/sessions/{sessionId}` | 获取某个已授权 session 的详情摘要 |
| GET | `/peer/sessions/{sessionId}/deltas` | 获取某个已授权 session 的增量 delta |
| POST | `/peer/subscriptions/request` | 请求订阅 |
| POST | `/peer/subscriptions/approve` | 同意订阅 |
| WS | `/peer/streams/session-deltas` | session delta 流 |
| POST | `/peer/confirmations` | 发送确认/疑问/忽略反馈 |

连接必须做最小认证。建议首次配对时展示双方短码：

```text
Alice: 482-913
Bob:   482-913
```

用户确认短码一致后保存 peer public key。

为了支持 `codex exec --sandbox read-only + curl` 的分析方式，MVP 至少需要提供 HTTP 只读接口，而不能只依赖 WebSocket 流。推荐接口语义：

#### `GET /peer/sessions`

返回当前 peer 允许订阅者看到的 session 摘要列表。必须经过项目白名单、label 分享规则、敏感信息检测和限长处理。

查询参数建议：

| 参数 | 说明 |
| --- | --- |
| projectId | 限定项目 |
| since | 只返回该时间之后活跃的 session |
| labels | 可选 label 过滤 |
| limit | 返回数量上限 |

#### `GET /peer/sessions/{sessionId}`

返回某个 session 的详情摘要，不返回原始完整 JSONL。该接口用于 Worker 在发现某个 session 相关后，通过 `curl` 拉取更多上下文。

响应字段建议：

| 字段 | 说明 |
| --- | --- |
| sessionId | Codex session id |
| projectId | 项目 |
| labels | 允许公开的 label |
| startedAt | session 起始时间 |
| latestRecordAt | 最新记录时间 |
| summaryMarkdown | 对外分享的 Markdown 摘要 |
| textExcerpt | 限长正文摘录 |
| pathsMentioned | 提到的路径 |
| commandsMentioned | 提到的命令 |
| gitRefs | 提到的 commit/branch/PR |
| redactionStatus | clean/redacted |

#### `GET /peer/sessions/{sessionId}/deltas`

返回某个 session 的 delta 列表，用于比 `summaryMarkdown` 更细粒度的分析。

查询参数建议：

| 参数 | 说明 |
| --- | --- |
| since | 只返回该时间之后的 delta |
| cursor | 断点续读 cursor |
| limit | 返回数量上限 |

所有 `/peer/sessions*` 接口都必须是只读接口。它们只暴露授权后的摘要或 delta，不暴露完整 JSONL、不暴露源码文件、不允许对端触发任何本机执行。

这里需要区分“传输方式”和“分享边界”：Codex Worker 可以通过 `curl` 主动访问 peer，但它访问的是协作 API，不是裸 session 文件。对端 API 必须负责裁剪输出，只返回授权后的 `summaryMarkdown`、`textExcerpt`、`SessionDelta`、路径/命令/git ref 等协作材料；不能因为调用方是 Codex Worker 就返回完整 JSONL 或完整 session 原文。

### 8.3 Session 监听

本机监听 `~/.codex/sessions` 或用户配置路径。

事件来源：

- 文件新增。
- 文件修改。
- 文件尾部追加。
- session metadata 更新。

实现建议：

1. 使用文件 watcher 捕获变更。
2. 对 JSONL 文件维护 offset。
3. 增量读取新行。
4. 每行 JSONL 保留原始 `timestamp`。
5. 检查 session label 是否满足 Share Policy。
6. 提取为 `SessionDelta`。
7. 本地先脱敏，再对订阅者广播。

关键点：不能只按文件 mtime 判断时间窗口。JSONL 每条记录都有 timestamp，协作分析必须按记录 timestamp 判断是否属于当前窗口。

另一个关键点：不能只按项目分享。即使某个项目在白名单内，未带分享 label 的 session 也不能外发。

### 8.4 JSONL 增量解析

Codex JSONL 示例：

```json
{
  "timestamp": "2026-05-14T12:58:28.523Z",
  "type": "response_item",
  "payload": {
    "type": "function_call_output",
    "call_id": "call_xxx",
    "output": "..."
  }
}
```

解析策略：

- `session_meta.payload.cwd` 用于项目归属。
- `response_item.payload.role=user/assistant` 用于对话上下文。
- `response_item.payload.type=function_call_output` 用于命令输出、测试结果、提交结果。
- `event_msg.payload.type=agent_message` 用于 Codex 中间状态和最终答复。
- `task_complete.last_agent_message` 可作为最终交付摘要的重要信号。

不要只取 JSONL 开头。长 session 必须支持尾部增量读取，否则会把很早创建的 session 误判成只有 AGENTS 指令。

### 8.5 分析 Worker

分析 Worker 使用和现有活动总结类似的 Codex 执行方式，核心形态是 `codex exec --sandbox read-only + curl`：

```bash
codex exec --ephemeral --sandbox read-only --add-dir <project_path> --output-last-message <file> -
```

MVP 阶段可以把分析 Worker 理解为“跨 peer 的活动总结”。它不必一开始输出结构化 JSON，也不必强制生成 `CollaborationHint[]`。第一版可以直接生成 Markdown 自由文本，类似现有活动总结能力，只要输出里标明关键证据即可。

对端 session 内容可以由 Worker 在 sandbox 内通过 `curl` 主动读取。应用层负责给 Worker 提供已配对 peer 的只读 endpoint、认证信息和访问范围；对端接口只返回已授权、已过滤、已脱敏、已限长的 session delta 或摘要。

订阅建立后，Worker 应先执行一次首次订阅基线分析，再进入增量分析。基线分析同样由 Codex Worker 在 sandbox 内通过受控 `curl` 完成：

1. 访问 `/peer/sessions?projectId=...&since=...` 获取当前可分享 session 摘要列表。
2. 对相关 session 访问 `/peer/sessions/{sessionId}` 获取详情摘要。
3. 必要时访问 `/peer/sessions/{sessionId}/deltas` 获取时间窗口内的细粒度 delta。
4. 将对端内容与本机 session 内容合并为一次跨 peer Markdown 协作总结。
5. 保存基线时间或 cursor，后续只针对新 delta 做增量更新。

输入包含：

- 自己当前项目 session delta。
- 已配对 peer 的只读协作 endpoint allowlist。
- 项目路径和可读目录。
- 时间窗口。
- 已知 git branch/commit。
- 用户设定的关注主题。

分析规则：

1. 只把落在时间窗口内的 JSONL record 计为当前进展。
2. 早于窗口的 record 只能作为背景。
3. 可以读取 `project_path` 下的代码和文档做只读验证。
4. 可以执行受控 `curl` 访问已配对 peer 暴露的协作接口，用于首次基线分析和后续增量分析。
5. `curl` 目标必须限制在已配对 peer 的 allowlist endpoint，例如 `/peer/projects`、`/peer/sessions`、`/peer/sessions/{sessionId}`、`/peer/sessions/{sessionId}/deltas`、`/peer/streams/session-deltas`。
6. 不允许执行构建、测试、安装、写文件、删除文件或访问未授权网络地址。
7. 输出可以是 Markdown 自由文本，但必须标明证据来源：peer、session、timestamp、文件路径。
8. 当后续需要提示卡片、去重、状态流转、确认反馈和证据跳转时，再把输出升级为结构化 `CollaborationHint[]`。

## 9. 旁路提示类型

### 9.1 边界重合风险

触发条件：

- 多人 session 同时提到同一文件、目录、模块、API、配置项。
- 多人工作分支改动主题相似。
- 对方 session 提到的文件与本机未提交改动重合。

提示示例：

```text
边界重合风险：Alice 和你都在处理 pkg/utils/gitops/templates.go。
Alice 的 session 在 2026-05-14T12:58Z 提到 node type scheduling from SKU。
你当前 session 正在讨论 schedulerName / nodeSelector。
建议先同步模板字段的最终归属。
```

### 9.2 功能协作提示

触发条件：

- 对方完成了一个你依赖的接口、配置、类型或文档。
- 对方的结论可作为你当前工作的输入。

提示示例：

```text
协作提示：Bob 已把 ResolvedBuildSpecSnapshot 作为 Drone adapter 前置条件。
如果你正在写 adapter，请先确认读取接口是否已收口。
```

### 9.3 冲突预警

触发条件：

- 两人对同一设计点给出相反结论。
- 对方删除/重命名了你 session 仍在引用的对象。
- 对方推送了你本地还未同步的分支或 commit。

### 9.4 互相确认

触发条件：

- 某条 session 产生影响其他人的架构结论。
- 某条 session 标记为“需要确认”。
- 分析 Worker 判断存在跨边界影响。

确认状态：

| 状态 | 说明 |
| --- | --- |
| confirm | 我确认这个结论 |
| question | 我有疑问 |
| conflict | 我认为有冲突 |
| ignore | 与我无关 |

## 10. UI 设计建议

### 10.1 侧边栏

新增区域：

```text
协作
  附近协作者
  协作总结
```

### 10.2 Peer 面板

展示：

- peer 名称。
- 在线状态。
- 公开项目。
- 是否已配对。
- 是否已订阅。
- 最近活动时间。

### 10.3 协作提示面板

提示卡片字段：

| 字段 | 说明 |
| --- | --- |
| severity | info/warning/critical |
| type | boundary/collaboration/conflict/confirmation |
| title | 简短提示 |
| summary | 具体说明 |
| evidence | peer、session、timestamp、路径 |
| actions | 确认、忽略、打开 session、复制提示词 |

### 10.4 旁路提示词注入

MVP 不自动写入用户当前 Codex session。更安全的方式是提供“复制旁路提示词”：

```text
请注意：Alice 在同项目 session 中已经修改/确认了 ...
请在继续实现前检查 ...
```

P1 可以支持用户显式点击“作为 Codex 提示词发送到当前 session”，但必须二次确认。

## 11. 数据模型

当前项目已有的核心模型集中在本机会话管理：

- backend：`SessionStatus`、`Session`、`SessionMeta`、`ArchiveRecord`、`MetadataFile`、`LabelCount`、`FilterCounts`。
- frontend：`Session`、`SessionsResponse`、`ScanResponse`、`ActivitySummaryResponse`、`SessionMutationResponse` 等 API 类型。

这些模型足够支撑扫描、预览、标注、备注、归档和活动总结，但还不能表达协作来源、增量游标、分享策略、脱敏结果、协作提示和 peer 关系。协作模块建议使用独立模型和独立持久化文件，不要把所有状态继续塞进现有 `metadata.json`。

### 11.1 PeerMetadata

```json
{
  "peerId": "peer_abc",
  "displayName": "Alice MacBook",
  "trusted": true,
  "publicKey": "...",
  "lastSeenAt": "2026-05-16T10:00:00Z"
}
```

### 11.2 Subscription

```json
{
  "subscriptionId": "sub_abc",
  "peerId": "peer_abc",
  "projectId": "project_dev_machine",
  "status": "active",
  "topics": ["boundary", "conflict", "confirmation"],
  "createdAt": "2026-05-16T10:00:00Z"
}
```

### 11.3 CollaborationHint

```json
{
  "hintId": "hint_abc",
  "type": "boundary",
  "severity": "warning",
  "projectId": "project_dev_machine",
  "title": "gitops template 边界重合",
  "summary": "Alice 和你都在处理 node type scheduling。",
  "evidence": [
    {
      "peerId": "peer_alice",
      "sessionId": "019e...",
      "timestamp": "2026-05-14T12:58:34Z",
      "path": "pkg/utils/gitops/templates.go"
    }
  ],
  "status": "unread",
  "createdAt": "2026-05-16T10:01:00Z"
}
```

### 11.4 MVP 所需新增模型

MVP 直接面向真实 LAN peer-to-peer 协作。以下模型是最小闭环需要的核心模型：

#### CollaborationSource

表示一个协作信号来源。MVP 主要来源是真实 LAN peer；本地模拟目录只作为开发调试手段。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| sourceId | string | 来源 ID |
| kind | localSimulated/lanPeer | 来源类型 |
| displayName | string | 展示名 |
| sessionRoot | string/null | 本地 session 根目录，LAN peer 可为空 |
| peerId | string/null | 真实 peer ID，本机模拟可为空 |
| enabled | boolean | 是否参与分析 |
| createdAt | ISO string | 创建时间 |

#### ProjectIdentity

表示项目归属。它比当前 `Session.projectPath` 更适合跨机器匹配。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| projectId | string | 稳定项目 ID |
| rootPath | string/null | 本机 repo root |
| pathLabel | string | 可展示名称 |
| gitRemoteHash | string/null | 脱敏后的 remote 标识 |
| gitBranch | string/null | 当前分支，可选公开 |

#### SessionDeltaCursor

保存 JSONL 增量读取进度，避免重复扫描大文件。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| sourceId | string | 来源 |
| sessionPath | string | JSONL 路径 |
| lastOffset | number | 已读取字节偏移 |
| lastRecordTimestamp | ISO string/null | 最新记录时间 |
| lastRecordHash | string/null | 最新记录 hash |
| updatedAt | ISO string | 更新时间 |

#### RedactionResult

表示脱敏结果。脱敏应该成为显式模型，而不是只做字符串替换。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| status | clean/redacted/blocked | 脱敏状态 |
| reasons | string[] | 命中的敏感类型 |
| redactedText | string | 脱敏后文本 |
| originalCharCount | number | 原始字符数 |
| redactedCharCount | number | 脱敏后字符数 |

#### CollaborationEvidence

`CollaborationHint.evidence` 的结构化证据项。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| sourceId | string | 来源 |
| peerId | string/null | LAN peer，可为空 |
| sessionId | string | Codex session id |
| deltaId | string/null | 关联 delta |
| timestamp | ISO string | 证据时间 |
| path | string/null | 关联文件路径 |
| excerpt | string | 证据摘录 |

#### CollaborationSummary

MVP 阶段的主要输出可以先是自由文本协作总结，而不是结构化提示卡片。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| summaryId | string | 总结 ID |
| projectId | string | 项目 |
| sourceIds | string[] | 涉及的协作来源 |
| markdown | string | Codex 生成的 Markdown 协作总结 |
| generatedAt | ISO string | 生成时间 |
| activeSince | ISO string | 时间窗口起点 |
| engine | string | 生成方式，例如 `codex-exec` |

### 11.5 LAN 协作阶段所需新增模型

真实 LAN 协作阶段再补以下模型：

| 模型 | 用途 |
| --- | --- |
| PeerPresence | mDNS 广播和发现 payload |
| ProjectPresence | 对外公开的项目活动 |
| PairingState | `discovered -> pairing -> paired -> rejected -> revoked` 的配对状态 |
| ConfirmationFeedback | 互相确认、疑问、冲突、忽略反馈 |

`Subscription` 已在 11.2 定义，但 LAN 阶段需要补充 `status` 的枚举语义，建议为 `requested`、`approved`、`active`、`paused`、`revoked`。

### 11.6 CollaborationStore

协作模块建议使用单独持久化文件，例如 `collaboration.json`：

```json
{
  "schemaVersion": 1,
  "localPeer": null,
  "sources": [],
  "trustedPeers": [],
  "projectPolicies": [],
  "subscriptions": [],
  "deltaCursors": [],
  "summaries": [],
  "hints": []
}
```

推荐初期不要持久化所有 `SessionDelta`。`SessionDelta` 可以作为可重建的中间产物；MVP 只持久化 `deltaCursors` 和 `CollaborationSummary`。后续如果做结构化提示卡片，再持久化 `CollaborationHint` 和必要 evidence，避免协作文件过快膨胀。

## 12. 隐私与安全

这是该功能的核心风险。

### 12.1 默认关闭

局域网协作必须默认关闭。用户需要主动开启：

```text
设置 -> 协作 -> 开启局域网发现
```

### 12.2 默认不分享全文

MVP 不直接分享完整 JSONL。分享顺序建议：

1. presence：我在哪个项目活跃。
2. delta 摘要：限长、脱敏后的记录。
3. 用户允许后，才分享更长上下文。

### 12.3 脱敏

发送前本地扫描：

- API key。
- token。
- password。
- private key。
- kubeconfig。
- `.env` 内容。
- 大段命令输出。

检测到高风险内容时：

```text
该 delta 疑似包含敏感信息，已停止分享。
```

### 12.4 项目白名单

用户必须按项目授权分享：

```text
允许分享：
- /Users/d-robotics/lab/dev-machine
- /Users/d-robotics/lab/ghost-kube

不允许分享：
- ~/.codex
- ~/private
```

### 12.5 Label 分享控制

项目白名单之后必须再经过 label 过滤。用户可以通过现有标签体系选择哪些 session 对外暴露。

推荐 UI：

```text
协作分享规则
  项目：/Users/d-robotics/lab/dev-machine
  分享包含以下 label 的 session：
    [x] share
    [x] team
    [x] review
  永不分享以下 label：
    [x] private
    [x] secret
  默认策略：
    ( ) 分享该项目全部 session
    (x) 只分享带有分享 label 的 session
    ( ) 默认不分享，逐条确认
```

规则顺序建议：

1. 项目必须在白名单内。
2. session 不能命中 `blockedLabels`。
3. `defaultShareMode=labeled` 时，session 必须命中 `requiredLabels`。
4. 发送前做敏感信息检测。
5. 首次分享某个 peer 前展示预览。

这样用户可以把个人探索、私密上下文、密钥排查等 session 留在本机，只把明确标记为协作材料的 session 暴露出去。

### 12.6 Curl 与分享粒度

允许 Codex Worker 使用 `curl` 不等于允许分享完整 session。MVP 的推荐策略是：

1. Codex Worker 可以通过受控 `curl` 访问已配对 peer 的只读协作 API。
2. 只读协作 API 只返回授权后的摘要、excerpt、delta、路径、命令和 git ref。
3. API 不返回完整 JSONL、不返回完整 session 原文、不返回未命中分享 label 的 session。
4. 命中 `private` 或 `secret` 的 session 不参与对外返回。
5. 命中敏感信息的 delta 必须先脱敏；高风险内容直接 blocked。
6. `curl` 使用短期访问凭据，凭据只对本次分析和 allowlist endpoint 有效。

### 12.7 配对认证

不能只靠 mDNS。mDNS 只负责发现，订阅必须配对确认。

## 13. MVP 范围

MVP 直接实现真实局域网 peer-to-peer 最小闭环。本机双目录模拟只作为开发调试手段，不作为产品能力设计。

1. 开关：开启/关闭局域网发现。
2. mDNS peer 发现。
3. peer 列表 UI。
4. 手动配对。
5. 项目级订阅。
6. label 级分享控制。
7. 对端只读协作 API，返回已授权、已过滤、已脱敏、已限长的 session delta 或摘要。
8. 首次订阅时执行一次全量基线分析，由 Codex Worker 通过受控 `curl` 拉取对端可分享 session 列表、详情摘要和必要 delta。
9. 基线完成后进入 JSONL 增量 tail。
10. delta 脱敏和限长。
11. 本地 `codex exec --sandbox read-only` 分析，并允许受控 `curl` 访问已配对 peer 的只读协作接口。
12. 协作总结面板，第一版展示 Markdown 自由文本。
13. 手动复制旁路提示词。

MVP 不做：

- 云同步。
- 自动注入 Codex。
- 对端远程执行。
- 全量 JSONL 分享。
- 代码文件同步。

## 14. 分阶段计划

| 阶段 | 目标 | 主要工作 | 验收 |
| --- | --- | --- | --- |
| 0. 设计验证 | 确认协作模型可行 | 明确隐私边界、发现协议、delta 模型 | 本文档评审通过 |
| 1. LAN 发现 | 实现 mDNS peer presence | 广播和发现 peer | 两台机器互相可见 |
| 2. 配对与订阅 | 建立可信 peer 关系 | 短码确认、项目订阅 | 订阅后可以访问对端只读协作 API |
| 3. 首次基线分析 | 生成初始协作总结 | Codex Worker 通过受控 curl 拉取对端可分享 session | 输出带证据的 Markdown 协作总结 |
| 4. 增量分析 | 持续更新协作总结 | JSONL tail、delta 脱敏、增量读取 | 新 delta 能更新总结 |
| 5. 安全强化 | 降低误分享风险 | 脱敏、项目白名单、label 分享控制、分享预览 | 默认安全可用 |

## 15. 关键技术风险

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| 敏感信息泄露 | 高 | 默认关闭、项目白名单、label 分享控制、脱敏、手动配对 |
| JSONL 太大 | 中 | offset tail、增量解析、限长 delta |
| AI 误判协作风险 | 中 | 证据引用、用户确认、低侵入展示 |
| mDNS 不稳定 | 中 | 手动输入 peer 地址作为 fallback |
| 多人状态不一致 | 低 | MVP 不追求强一致，只做本地提示 |
| 自动提示打扰 | 中 | severity、静默模式、按 topic 订阅 |

## 16. 当前默认决策与待确认问题

### 16.1 当前默认决策

1. MVP 直接做真实局域网 peer-to-peer 最小闭环，不再拆 MVP-A/MVP-B。
2. 不做组内协作，不设计订阅组。
3. 分析 Worker 使用 `codex exec --sandbox read-only + curl`。
4. 首次订阅先做一次全量基线分析，再进入增量分析。
5. 全量基线分析也由 Codex Worker 通过受控 `curl` 拉取对端只读协作 API。
6. Codex Worker 可以 `curl`，但只能访问已配对 peer 的 allowlist endpoint。
7. 对端 API 只返回授权后的摘要、excerpt、delta、路径、命令和 git ref，不返回完整 JSONL 或完整 session 原文。
8. MVP 输出 Markdown 协作总结，不强制结构化 `CollaborationHint[]`。
9. `CollaborationHint`、提示卡片、确认反馈和复杂去重放到后续阶段。
10. 协作状态持久化建议使用独立 `collaboration.json`，不要塞进现有 `metadata.json`。

### 16.2 仍待确认问题

1. 是否需要支持跨网段/VPN，还是严格局域网？
2. 订阅请求是否需要对方每次确认，还是信任 peer 后自动允许同项目？
3. 是否要把 git remote hash 作为 `projectId` 的主要依据？
4. 是否允许读取对方发送的文件路径名，还是也要路径脱敏？
5. 默认分享 label 是否固定为 `share`、`team`、`review`，还是允许用户自定义？
6. 是否需要把自动项目 label 和手动分享 label 区分开，避免用户误以为项目 label 会触发分享？
7. 短期 curl token 的有效期、作用域和刷新机制如何定义？
8. HTTP polling + cursor 是否足够，还是 MVP 必须实现 WebSocket delta stream？

## 17. 推荐结论

建议把该能力作为 P2/P3 的独立协作模块推进，不要混进当前个人 session 管理 MVP。产品 MVP 直接做真实局域网 peer-to-peer 最小闭环：

1. 局域网发现 peer。
2. 手动配对和项目级订阅。
3. 对端暴露只读协作 API。
4. 首次订阅时由 Codex Worker 通过受控 `curl` 做全量基线分析。
5. 后续通过 JSONL delta 做增量分析。
6. UI 展示 Markdown 协作总结。

本机双目录模拟可以作为开发调试和自动化测试 fixture，但不作为面向用户的产品能力。

## 18. 当前项目缺口检查

对照当前代码，确实缺失的内容如下。

### 18.1 模型缺口

| 缺口 | 当前状态 | 建议优先级 |
| --- | --- | --- |
| CollaborationSource | 不存在 | P0，MVP 需要 |
| ProjectIdentity | 只有 `Session.projectPath`，没有跨机器稳定项目 ID | P0，MVP 需要 |
| SessionDelta | 不存在 | P0，MVP 需要 |
| SessionDeltaCursor | 不存在，目前扫描偏向一次性读取 | P0，MVP 需要 |
| SharePolicy | 不存在，当前 label 只是普通整理标签 | P0，MVP 需要 |
| RedactionResult | 不存在 | P0，MVP 需要 |
| CollaborationSummary | 不存在 | P0，MVP 需要 |
| CollaborationHint | 文档有示例，代码没有 | P1，结构化提示卡片阶段需要 |
| CollaborationEvidence | 只在示例里内嵌，未独立定义 | P1，结构化提示卡片阶段需要 |
| CollaborationStore | 不存在，现有 `MetadataFile` 不适合承载协作状态 | P0，MVP 需要 |
| PeerMetadata/PeerPresence/ProjectPresence | 不存在 | P1，LAN 阶段需要 |
| PairingState | 不存在 | P1，LAN 阶段需要 |
| Subscription | 不存在 | P1，LAN 阶段需要 |
| ConfirmationFeedback | 不存在 | P2，互相确认阶段需要 |

### 18.2 服务和实现缺口

| 缺口 | 说明 | 建议优先级 |
| --- | --- | --- |
| JSONL tail reader | 当前 scanner 可以解析 session，但没有按 offset 持续读取增量 | P0 |
| Delta extractor | 需要把 JSONL record 归一化为 `SessionDelta` | P0 |
| 路径/命令/git ref 提取器 | 协作风险不能完全依赖 LLM，应先做确定性提取 | P0 |
| 脱敏器 | 分享前必须检测 API key、token、private key、kubeconfig、`.env` 等 | P0 |
| 协作提示生成器 | 先实现规则版 overlap，再接 LLM 解释 | P0 |
| collaboration.json 存储 | 独立保存协作配置、游标和提示 | P0 |
| mDNS discovery | 当前没有 LAN 发现服务 | P1 |
| peer HTTP/WS API | 当前只有本机 `/api/*`，没有 `/peer/*` | P1 |
| 配对认证 | 需要短码、public key 保存、撤销信任 | P1 |
| WebSocket delta stream | 真实 LAN 订阅阶段需要 | P1 |
| 互相确认回传 | 文档有用户故事，当前没有 API 和模型 | P2 |

### 18.3 文档仍需补充的设计点

1. 明确 `projectId` 生成规则：repo root、git remote hash、无 git 项目的 fallback。
2. 明确 label 来源：现有手动 label 是否直接复用，还是新增协作专用 label 命名空间。
3. 明确 label 后改行为：session 后续加上 `private` 或 `secret` 时，是否停止分享、是否清理已生成提示。
4. 明确脱敏阻断规则：哪些命中只 redacted，哪些直接 blocked。
5. 明确 Codex worker 可访问的 peer allowlist endpoint，避免任意网络访问。
6. 明确对端只读协作 API 的返回格式：先支持 Markdown/摘要和 delta 列表，后续再考虑结构化 hint schema。
7. 明确自由文本协作总结的最小证据要求：必须包含 peer、session、timestamp、文件路径或命令。
8. 明确提示去重策略：同一文件/同一 peer/同一时间窗口是否合并。
9. 明确提示生命周期：`unread`、`read`、`dismissed`、`resolved` 是否足够。
10. 明确 peer key 轮换、撤销信任、重复 peerId 的处理。
11. 明确 prompt injection 防护：peer delta 只能作为不可信证据，不能覆盖本地系统指令和分享策略。
12. 明确 MVP 验收样例：准备两组 fixture JSONL 作为开发测试数据，至少覆盖路径重合、结论冲突、敏感内容阻断。
