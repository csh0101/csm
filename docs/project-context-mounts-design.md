# Project Context Mounts 详细设计稿

## 1. 背景

当前 Traceway 已经具备本地 Codex session 扫描、预览、标注、活动总结和局域网协作感知能力。下一阶段可以进一步从“会话管理器”扩展为“项目上下文数据平面”：把项目关联的外部资源以只读文件系统的形式挂载到项目目录下，让 Codex CLI 和开发者都能通过 `ls`、`cat`、`grep`、`jq` 等通用文件操作读取上下文。

该设计的第一个落地点是 MySQL。很多项目真实业务上下文并不只在源码里，还在数据库 schema、索引、示例数据和常用查询结果里。如果这些信息只能通过数据库客户端、SQL 控制台或应用代码间接理解，Codex 很容易缺少关键事实。把数据库挂载成受控目录后，Codex 可以在只读 sandbox 中像读取普通文件一样读取 schema 和 sample data。

## 2. 产品定位

功能名称建议为 **Project Context Mounts**，中文可称为“项目上下文挂载”。

核心定位：

> 为每个项目注入一组受控、只读、可审计的虚拟文件系统，把数据库等外部资源映射成 Codex 和人都容易读取的目录树。

它不是数据库管理工具，不替代 DataGrip、DBeaver、MySQL CLI，也不是把生产数据完整暴露给 AI。它只提供经过策略限制、脱敏、限量和缓存的项目上下文视图。

## 3. 目标用户

| 用户角色 | 核心诉求 | 典型场景 |
| --- | --- | --- |
| Codex 高频用户 | 让 Codex 快速理解项目连接的数据库结构 | 修复接口字段错误、理解业务表关系、生成迁移脚本草稿 |
| 后端开发者 | 不打开数据库客户端也能快速查看 schema 和 sample | 查看表结构、索引、字段注释、少量样例数据 |
| Tech Lead | 在协作分析中把数据库事实作为证据 | 判断多人变更是否影响同一张表或同一类业务数据 |
| 工具管理员 | 控制数据库访问权限和审计 | 强制只读账号、限制样例行数、记录访问历史 |

## 4. 非目标范围

1. 不在 MVP 中支持写数据库。
2. 不在 MVP 中提供通用 SQL IDE。
3. 不默认读取生产库，生产连接必须显式确认。
4. 不全量导出数据库数据。
5. 不绕过数据库自身权限；挂载进程只能使用用户配置的数据库账号。
6. 不把凭证写入项目目录或 session 内容。
7. 不引入第三方虚拟文件系统框架作为核心依赖；本设计只借鉴“把外部资源映射成文件树”的产品思想。

## 5. 总体架构

```text
Traceway App
  ├─ React UI
  │   ├─ Project Context Mounts 面板
  │   ├─ 挂载状态 / 健康检查 / 错误提示
  │   └─ Codex 提示生成
  ├─ Rust Backend
  │   ├─ Project Detector
  │   ├─ Credential Vault
  │   ├─ Mount Registry
  │   ├─ Mount Manager
  │   ├─ Policy Engine
  │   ├─ Connector Layer
  │   ├─ Cache Store
  │   └─ Audit Log
  ├─ Rust FUSE Sidecar
  │   ├─ Path Router
  │   └─ File Renderer
  └─ Project Mountpoints
      └─ <project>/.traceway/mounts/<mount-id>/
```

MVP 推荐用 Rust 实现 FUSE sidecar，优先使用 `fuser`。原因是当前后端已经是 Rust，项目配置、metadata、协作状态、调度和本地存储都在 Rust 侧，FUSE 挂载如果也在 Rust 内实现，能减少运行时和打包复杂度。

## 6. 核心概念

### 6.1 Project

Project 表示一个可识别的本地项目。可复用现有协作设计中的 project identity 逻辑。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| projectId | string | 稳定项目 ID，建议由 repo root、remote URL hash、路径 hash 组合生成 |
| rootPath | string | 项目本地根目录 |
| pathLabel | string | UI 展示名称 |
| gitRemoteHash | string/null | 可选，用于跨机器识别同一项目 |
| gitBranch | string/null | 当前分支，可选 |

### 6.2 Mount

Mount 表示某个项目下的一个上下文挂载实例。

同一个项目可以挂多个数据源，同一个机器也可以同时挂多个项目。多 MySQL 的核心隔离规则是：

```text
mount_key = project_id + mount_id
```

因此 Project A 和 Project B 都可以拥有同名 `mysql-main` 挂载，但它们在 registry 中是两条独立记录。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| mountId | string | 项目内唯一，如 `mysql-main`、`mysql-analytics` |
| projectId | string | 所属项目 |
| connectorKind | mysql/postgres/redis/s3/logs | 连接器类型 |
| displayName | string | UI 展示名 |
| mountPoint | string | 实际 FUSE 挂载路径 |
| credentialProfileId | string | 凭证 profile 引用 |
| policyId | string | 策略引用 |
| status | stopped/starting/running/error | 当前状态 |
| lastHealthCheckAt | ISO string/null | 最近健康检查时间 |
| lastError | string/null | 最近错误 |
| createdAt | ISO string | 创建时间 |
| updatedAt | ISO string | 更新时间 |

### 6.3 Credential Profile

Credential Profile 存储数据库连接凭证。它必须保存在 Traceway 的本地受控数据目录或系统 keychain 中，不能写入项目目录。

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| profileId | string | 凭证 ID |
| projectId | string | 所属项目，可选限制 |
| kind | mysql | 凭证类型 |
| displayName | string | 展示名 |
| redactedDsn | string | 脱敏后的连接字符串 |
| encryptedDsn | string | 加密后的真实连接字符串 |
| createdAt | ISO string | 创建时间 |
| updatedAt | ISO string | 更新时间 |

MVP 可以先使用本地文件加密；桌面产品化阶段优先接入 macOS Keychain、Windows Credential Manager 和 Linux Secret Service。

### 6.4 Policy

Policy 控制挂载暴露的数据范围。

| 字段 | 类型 | 默认值 | 说明 |
| --- | --- | --- | --- |
| readonly | boolean | true | MVP 必须只读 |
| allowedSchemas | string[] | [] | 为空表示全部 schema 可见，后续可改为默认空且用户选择 |
| blockedSchemas | string[] | ["mysql", "performance_schema", "information_schema", "sys"] | 默认隐藏系统库 |
| allowedTables | string[] | [] | 表白名单 |
| blockedTables | string[] | [] | 表黑名单 |
| maxSampleRows | number | 100 | 单表 sample 最大行数 |
| maxLookupRows | number | 100 | 单次索引 lookup 最大行数 |
| maxFileBytes | number | 1048576 | 单个虚拟文件最大输出字节数 |
| queryTimeoutMs | number | 3000 | 单次数据库查询超时 |
| redactColumns | string[] | ["password", "token", "secret", "email", "phone"] | 命中字段名时脱敏 |
| requireTenantFilter | boolean | false | 是否强制 lookup 带租户过滤 |
| tenantColumns | string[] | ["tenant_id", "org_id", "workspace_id"] | 多租户字段候选 |
| allowAddressableLookups | boolean | true | 是否允许按路径参数触发受控 lookup |
| allowCustomQueries | boolean | false | MVP 默认不开放自定义查询 |

## 7. 多项目多 MySQL 模型

### 7.1 目录可同名

Project A：

```text
/repo/project-a/.traceway/mounts/mysql-main/
```

Project B：

```text
/repo/project-b/.traceway/mounts/mysql-main/
```

两个目录名可以相同，因为 FUSE sidecar 不是只根据 `mysql-main` 判断连接，而是根据 mountpoint 反查 Mount Registry。

### 7.2 Registry 记录示例

```json
{
  "mounts": [
    {
      "projectId": "project_a",
      "mountId": "mysql-main",
      "mountPoint": "/repo/project-a/.traceway/mounts/mysql-main",
      "credentialProfileId": "profile_a",
      "policyId": "policy_a"
    },
    {
      "projectId": "project_b",
      "mountId": "mysql-main",
      "mountPoint": "/repo/project-b/.traceway/mounts/mysql-main",
      "credentialProfileId": "profile_b",
      "policyId": "policy_b"
    }
  ]
}
```

### 7.3 读取路由

当 Codex 执行：

```bash
cat /repo/project-a/.traceway/mounts/mysql-main/schemas/app/tables/users/schema.sql
```

FUSE sidecar 的路由流程是：

1. 根据文件系统请求定位 mountpoint。
2. 用 mountpoint 查询 Mount Registry。
3. 找到 `project_a + mysql-main`。
4. 读取 `profile_a` 和 `policy_a`。
5. 获取或创建 MySQL connector。
6. 查询 Project A 的 MySQL。
7. 渲染 `schema.sql` 返回。

同名路径在 Project B 下会命中 `project_b + mysql-main`，不会串库。

## 8. 挂载目录设计

Project Context Mounts 不是全量挂载，也不会把数据库每一行 materialize 成真实文件。它是一个虚拟、懒加载、按需生成的只读文件系统：

```text
目录结构表达“可读取能力”
文件内容在 ls/cat 时由 FUSE sidecar 即时生成或读取缓存
```

因此，`lookup/by-primary/id/123.json` 这类路径不需要提前存在，也不会在 `readdir` 中列出所有可能的 ID。Codex 可以直接 `cat` 一个地址型虚拟文件，FUSE sidecar 根据路径参数执行受控查询。

MVP 建议每个 MySQL mount 暴露如下能力目录：

```text
.traceway/
  mounts/
    mysql-main/
      README.md
      connection.json
      health.json
      schemas/
        app/
          schema.json
          tables/
            users/
              schema.sql
              columns.json
              indexes.sql
              foreign_keys.json
              inferred_relations.json
              lookup_manifest.json
              count.txt
              sample.jsonl
              stats/
                status_counts.json
                null_counts.json
                top_values/
                  README.md
              lookup/
                README.md
                by-primary/
                  id/
                    {value}.json
                by-unique/
                  email/
                    {value}.json
                by-index/
                  tenant_id/
                    {value}.jsonl
            orders/
              schema.sql
              columns.json
              indexes.sql
              foreign_keys.json
              inferred_relations.json
              lookup_manifest.json
              count.txt
              sample.jsonl
      queries/
        README.md
```

其中 `{value}.json` 和 `{value}.jsonl` 是地址型虚拟文件，不会被 `ls` 展开；用户或 Codex 需要根据 `lookup_manifest.json` 里的模板直接读取。

### 8.1 README.md

说明该挂载是只读数据库上下文，列出安全边界和推荐读取顺序。

示例：

```markdown
# MySQL Context Mount

This is a read-only Traceway context mount.

Recommended order:
1. Read `connection.json`.
2. Inspect `schemas/<db>/tables/<table>/schema.sql`.
3. Read `columns.json`, `indexes.sql`, `foreign_keys.json`, and `inferred_relations.json`.
4. Read `lookup_manifest.json` to understand safe lookup paths.
5. Use `sample.jsonl` only as limited sample data.
6. Use lookup paths for targeted reads, for example `lookup/by-primary/id/123.json`.

Do not assume `sample.jsonl` contains complete table data.
This mount does not support arbitrary SQL or arbitrary JOIN in MVP.
```

### 8.2 connection.json

只展示脱敏信息。

```json
{
  "kind": "mysql",
  "host": "127.0.0.1",
  "port": 3306,
  "database": "app",
  "user": "readonly_user",
  "readonly": true,
  "source": "project .env",
  "redacted": true
}
```

### 8.3 schema.sql

来自 `SHOW CREATE TABLE` 或等价 information schema 组装结果。

### 8.4 columns.json

结构化列信息，便于 Codex 可靠读取。

```json
[
  {
    "name": "id",
    "type": "bigint unsigned",
    "nullable": false,
    "key": "PRI",
    "default": null,
    "comment": "primary key"
  }
]
```

### 8.5 sample.jsonl

默认最多 `maxSampleRows` 行。字段值必须经过脱敏。

```jsonl
{"id":1,"email":"[redacted-email]","created_at":"2026-05-17T08:00:00Z"}
{"id":2,"email":"[redacted-email]","created_at":"2026-05-17T08:10:00Z"}
```

`sample.jsonl` 只用于理解数据形态，不保证包含 Codex 想排查的具体记录。

### 8.6 foreign_keys.json

来自数据库真实外键约束。用于帮助 Codex 分步读取相关表，而不是直接执行 JOIN。

```json
[
  {
    "column": "user_id",
    "references": {
      "schema": "app",
      "table": "users",
      "column": "id"
    }
  }
]
```

### 8.7 inferred_relations.json

很多 MySQL 项目没有真实 foreign key，只靠字段命名约定维护关系。Traceway 可以根据字段名、索引和表名推断弱关系，例如 `orders.user_id -> users.id`。

```json
[
  {
    "confidence": "medium",
    "column": "user_id",
    "references": {
      "schema": "app",
      "table": "users",
      "column": "id"
    },
    "reason": "column name matches users.id convention"
  }
]
```

推断关系必须标明 `confidence` 和 `reason`，Codex 不能把它当成数据库约束事实。

### 8.8 lookup_manifest.json

`lookup_manifest.json` 告诉 Codex 哪些地址型路径可以直接读取。它是替代任意 SQL 和任意 JOIN 的核心机制。

```json
[
  {
    "name": "user_by_id",
    "pathTemplate": "lookup/by-primary/id/{value}.json",
    "queryShape": "primary-key",
    "maxRows": 1
  },
  {
    "name": "orders_by_user_id",
    "pathTemplate": "../orders/lookup/by-index/user_id/{value}.jsonl",
    "queryShape": "index-lookup",
    "maxRows": 100
  }
]
```

Codex 的推荐读取方式是：

```bash
cat schemas/app/tables/users/lookup_manifest.json
cat schemas/app/tables/users/lookup/by-primary/id/123.json
cat schemas/app/tables/orders/lookup/by-index/user_id/123.jsonl
```

### 8.9 stats 目录

有些问题不需要读取具体行，而是需要知道状态分布、空值比例、常见枚举值。`stats/` 目录提供受控聚合视图。

MVP 可以先支持：

```text
stats/status_counts.json
stats/null_counts.json
stats/top_values/<column>.json
```

限制：

1. 只对低基数字段生成 `top_values`，例如 `status`、`type`、`state`。
2. 禁止对 `email`、`phone`、`token` 等高敏感或高基数字段生成 top values。
3. 聚合查询必须有超时和缓存。

### 8.10 不支持任意 JOIN

MVP 不支持任意 SQL，也不支持任意 JOIN。Codex 通过 `foreign_keys.json`、`inferred_relations.json` 和 `lookup_manifest.json` 分步读取相关数据：

```text
1. 读取 users/schema.sql 和 users/lookup_manifest.json。
2. 读取 users/lookup/by-primary/id/123.json。
3. 根据 relations 或 manifest 发现 orders.user_id。
4. 读取 orders/lookup/by-index/user_id/123.jsonl。
5. 在 Codex 上下文中关联两份小结果。
```

后续如果确实需要 JOIN，只能作为白名单 query template，由用户或项目配置显式开启：

```text
queries/user_order_summary/
  README.md
  params.schema.json
  result/user_id=123.json
```

白名单 query template 不属于 MVP。

## 9. 创建挂载时序

```mermaid
sequenceDiagram
  autonumber
  participant User as 用户
  participant UI as Traceway UI
  participant API as Rust Backend API
  participant Detector as Project Detector
  participant Vault as Credential Vault
  participant Registry as Mount Registry
  participant Manager as Mount Manager
  participant Fuse as FUSE Sidecar
  participant FS as 项目挂载目录

  User->>UI: 在项目中添加 MySQL 挂载
  UI->>API: POST /api/projects/{projectId}/mounts/mysql
  API->>Detector: 扫描 .env / compose / ORM 配置
  Detector-->>API: 返回候选 DSN，敏感字段脱敏
  API-->>UI: 展示候选连接
  User->>UI: 确认连接、策略、挂载路径
  UI->>API: POST /api/projects/{projectId}/mounts/mysql/confirm
  API->>Vault: 加密保存真实 DSN
  API->>Registry: 保存 MountRecord
  API->>Manager: start projectId + mountId
  Manager->>Fuse: 注册 MountContext
  Fuse->>FS: mount .traceway/mounts/{mountId}
  Manager-->>API: status=running
  API-->>UI: 挂载完成
```

## 10. 读取虚拟文件时序

```mermaid
sequenceDiagram
  autonumber
  participant Codex as Codex 或开发者
  participant FS as FUSE 挂载目录
  participant Fuse as FUSE Sidecar
  participant Router as Path Router
  participant Registry as Mount Registry
  participant Policy as Policy Engine
  participant Cache as Cache Store
  participant Conn as MySQL Connector
  participant DB as MySQL

  Codex->>FS: cat schemas/app/tables/users/schema.sql
  FS->>Fuse: read(path)
  Fuse->>Router: resolve mountpoint + virtual path
  Router->>Registry: lookup projectId + mountId
  Registry-->>Router: MountContext
  Router->>Policy: authorize virtual file read
  Policy-->>Router: allowed with limits
  Router->>Cache: get schema cache
  alt cache hit
    Cache-->>Router: schema.sql
  else cache miss
    Router->>Conn: get table schema
    Conn->>DB: SHOW CREATE TABLE app.users
    DB-->>Conn: result
    Conn-->>Router: schema.sql
    Router->>Cache: save schema cache
  end
  Router-->>Fuse: bytes
  Fuse-->>FS: bytes
  FS-->>Codex: schema.sql
```

## 11. API 设计草案

### 11.1 发现项目数据源

```http
POST /api/projects/{projectId}/mounts/mysql/discover
```

响应：

```json
{
  "projectId": "project_a",
  "candidates": [
    {
      "source": ".env",
      "kind": "mysql",
      "redactedDsn": "mysql://readonly_user@127.0.0.1:3306/app",
      "confidence": "high"
    }
  ]
}
```

### 11.2 创建挂载

```http
POST /api/projects/{projectId}/mounts
```

请求：

```json
{
  "connectorKind": "mysql",
  "mountId": "mysql-main",
  "displayName": "Main MySQL",
  "dsn": "mysql://readonly_user:password@127.0.0.1:3306/app",
  "mountPointMode": "project",
  "policy": {
    "readonly": true,
    "maxSampleRows": 100,
    "maxFileBytes": 1048576,
    "queryTimeoutMs": 3000
  }
}
```

响应：

```json
{
  "mount": {
    "projectId": "project_a",
    "mountId": "mysql-main",
    "mountPoint": "/repo/project-a/.traceway/mounts/mysql-main",
    "status": "stopped"
  }
}
```

### 11.3 启动挂载

```http
POST /api/projects/{projectId}/mounts/{mountId}/start
```

### 11.4 停止挂载

```http
POST /api/projects/{projectId}/mounts/{mountId}/stop
```

### 11.5 查询挂载状态

```http
GET /api/projects/{projectId}/mounts
GET /api/projects/{projectId}/mounts/{mountId}
```

### 11.6 更新策略

```http
PATCH /api/projects/{projectId}/mounts/{mountId}/policy
```

策略更新后需要使相关 cache 失效。

## 12. 后端模块设计

### 12.1 Project Detector

职责：

1. 识别项目根目录。
2. 扫描 `.env`、`.env.local`、`docker-compose.yml`、Prisma、TypeORM、Sequelize、Django、Rails、Laravel 等常见配置。
3. 提取候选 DSN。
4. 对候选 DSN 脱敏。
5. 给出可信度。

MVP 可以先支持：

- `.env` 中的 `DATABASE_URL`
- `.env` 中的 `MYSQL_HOST`、`MYSQL_PORT`、`MYSQL_DATABASE`、`MYSQL_USER`
- `docker-compose.yml` 中的 MySQL service

### 12.2 Mount Registry

职责：

1. 持久化 mount records。
2. 根据 mountpoint 反查 `projectId + mountId`。
3. 提供 list/status 接口。
4. 保证同一 mountpoint 不被重复挂载。

建议持久化到新的 `mounts.json`，后续可迁移到 SQLite。

### 12.3 Credential Vault

职责：

1. 保存真实 DSN。
2. 对 UI 和虚拟文件只输出脱敏连接信息。
3. 支持删除、轮换和健康检查。

MVP 本地加密即可；产品化阶段使用系统 credential store。

### 12.4 Mount Manager

职责：

1. 启动和停止 FUSE sidecar。
2. 注册 MountContext。
3. 处理进程崩溃和自动恢复。
4. 做健康检查。
5. 在 app 退出时 unmount。

MVP 可以每个 mount 一个 FUSE daemon。后续如果 FUSE 库和平台行为允许，再优化为单进程管理多个 mountpoint。

### 12.5 Policy Engine

职责：

1. 判断虚拟路径是否可读。
2. 限制 schema、table、sample 行数、输出字节数。
3. 对敏感字段脱敏。
4. 阻止系统库和高风险表。
5. 控制查询超时。
6. 控制地址型 lookup 是否允许、最大行数和是否必须带租户过滤。
7. 阻止 MVP 中的任意 SQL 和任意 JOIN。

任何 connector 返回的数据都必须经过 Policy Engine。

### 12.6 Connector Layer

提供统一接口：

```rust
trait ContextConnector {
    fn health_check(&self) -> Result<HealthStatus>;
    fn list_schemas(&self) -> Result<Vec<SchemaInfo>>;
    fn list_tables(&self, schema: &str) -> Result<Vec<TableInfo>>;
    fn table_schema_sql(&self, schema: &str, table: &str) -> Result<String>;
    fn table_columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnInfo>>;
    fn table_indexes(&self, schema: &str, table: &str) -> Result<Vec<IndexInfo>>;
    fn table_foreign_keys(&self, schema: &str, table: &str) -> Result<Vec<ForeignKeyInfo>>;
    fn inferred_relations(&self, schema: &str, table: &str) -> Result<Vec<InferredRelation>>;
    fn lookup_manifest(&self, schema: &str, table: &str, policy: &MountPolicy) -> Result<Vec<LookupTemplate>>;
    fn table_count(&self, schema: &str, table: &str) -> Result<Option<u64>>;
    fn sample_rows(&self, schema: &str, table: &str, limit: usize) -> Result<Vec<JsonRow>>;
    fn lookup_rows(&self, request: LookupRequest) -> Result<Vec<JsonRow>>;
    fn table_stats(&self, request: StatsRequest) -> Result<JsonValue>;
}
```

实际 Rust trait 需要根据异步 runtime 选择 `async_trait` 或同步接口加后台 task。FUSE 回调通常是同步模型，因此建议将 DB 查询与缓存刷新放入独立任务，FUSE read 优先读缓存；允许 cache miss 时同步等待有限时间。

## 13. FUSE 设计

### 13.1 只读文件系统

MVP 只支持：

- `lookup`
- `getattr`
- `readdir`
- `open`
- `read`

写相关操作全部返回只读错误：

- `write`
- `create`
- `mkdir`
- `unlink`
- `rename`
- `setattr`

### 13.2 虚拟 inode

FUSE 需要稳定 inode。可以基于 mount context 和 virtual path 生成 inode map。

```text
inode = stable_hash(project_id + mount_id + virtual_path)
```

为了避免 hash collision，运行时仍应维护 path 到 inode 的双向表。

### 13.3 文件内容生成

虚拟文件内容来源分三类：

1. 静态内容：`README.md`
2. 配置内容：`connection.json`、`health.json`
3. 数据内容：`schema.sql`、`columns.json`、`indexes.sql`、`foreign_keys.json`、`inferred_relations.json`、`lookup_manifest.json`、`count.txt`、`sample.jsonl`、`stats/*.json`、`lookup/**/*.jsonl`

所有内容都需要设置最大字节数，超限时截断并追加说明。

### 13.4 虚拟节点类型

FUSE 文件树分三类节点：

| 类型 | 是否可枚举 | 示例 | 说明 |
| --- | --- | --- | --- |
| enumerable nodes | 是 | `schemas/`、`tables/` | 可以 `ls`，但需要数量限制和分页/索引策略 |
| virtual fixed files | 是 | `schema.sql`、`columns.json`、`lookup_manifest.json` | 固定文件名，内容按需生成 |
| addressable virtual files | 否 | `lookup/by-primary/id/123.json` | 不在 `readdir` 中列出，但可以直接 `cat` |

地址型虚拟文件用于表达“按参数读取”的能力，避免路径爆炸。例如：

```text
lookup/by-primary/id/{value}.json
lookup/by-index/user_id/{value}.jsonl
```

`ls lookup/by-primary/id/` 不应列出所有 ID，只返回 README 或空目录；`cat lookup/by-primary/id/123.json` 才会触发受控查询。

### 13.5 非全量挂载

挂载目录不是数据库快照，不预先创建所有库、表、行或 lookup 结果。任何可能无限增长的路径都必须设计成不可枚举的地址型路径。

禁止设计：

```text
tables/users/rows/1.json
tables/users/rows/2.json
tables/users/rows/3.json
```

推荐设计：

```text
tables/users/lookup/by-primary/id/{value}.json
```

这样 Codex 可以按意图读取目标记录，但不能通过 `find` 枚举整张表。

## 14. 缓存策略

| 内容 | 默认 TTL | 说明 |
| --- | --- | --- |
| schemas list | 60s | 库列表 |
| tables list | 60s | 表列表 |
| schema.sql | 5m | 表结构变化不频繁 |
| columns.json | 5m | 表字段 |
| indexes.sql | 5m | 索引 |
| count.txt | 30s | 可能较慢，可允许返回估算或 disabled |
| sample.jsonl | 60s | 样例数据 |
| lookup result | 30s | 按参数读取的小结果 |
| stats result | 60s | 聚合统计结果 |

策略变更、凭证变更、手动刷新、健康检查失败时应清理相关缓存。

## 15. 安全与隐私

### 15.1 数据库账号

强制建议使用只读账号。UI 在创建挂载时需要提示：

- 不建议使用 root。
- 不建议连接生产库。
- 如果必须连接生产库，必须启用更严格的 sample 和脱敏策略。

### 15.2 脱敏

字段名命中以下规则时默认脱敏：

```text
password
passwd
pwd
secret
token
api_key
apikey
email
phone
mobile
address
id_card
ssn
```

脱敏后的 sample 应保留类型和形态信息，例如：

```json
{
  "email": "[redacted-email]",
  "phone": "[redacted-phone]",
  "password_hash": "[redacted-secret]"
}
```

### 15.3 审计

记录每次虚拟文件读取：

| 字段 | 说明 |
| --- | --- |
| timestamp | 访问时间 |
| projectId | 项目 |
| mountId | 挂载 |
| virtualPath | 虚拟路径 |
| result | success/error/blocked |
| bytesReturned | 返回字节数 |
| durationMs | 耗时 |
| error | 错误信息 |

MVP 不需要识别是 Codex 还是人访问，因为文件系统请求通常不可靠携带上层进程身份。后续可以在 macOS/Linux 上尝试通过 FUSE request context 关联 PID，再解析进程名。

## 16. 与 Codex 的交互

UI 提供“一键复制 Codex 提示”：

```text
这个项目有一个只读 MySQL 上下文挂载：

.traceway/mounts/mysql-main

请优先读取：
1. README.md
2. connection.json
3. schemas/<db>/tables/<table>/schema.sql
4. columns.json
5. indexes.sql
6. foreign_keys.json 和 inferred_relations.json
7. lookup_manifest.json
8. sample.jsonl
9. lookup/by-primary 或 lookup/by-index 下的地址型虚拟文件

sample.jsonl 只是限量样例数据，不代表完整表内容。
如果需要具体记录，请根据 lookup_manifest.json 分步读取受控 lookup 路径。
MVP 不支持任意 SQL 或任意 JOIN；请通过 relations + lookup 分步关联结果。
不要尝试写入挂载目录。
```

如果 `codex exec --sandbox read-only` 只允许读当前项目，则 mountpoint 应优先放在项目目录内：

```text
<project>/.traceway/mounts/<mount-id>
```

如果 mountpoint 放在 app data 目录，则需要明确把该路径加入 Codex 可读上下文，否则 sandbox 可能无法访问。

## 17. 设计来源与边界

本设计借鉴“面向 AI Agent 的虚拟文件系统”思想：把数据库、对象存储、日志等外部资源暴露为可读文件树，让 Codex 通过普通文件操作理解项目上下文。

但 Traceway 不依赖外部虚拟文件系统框架作为核心架构。原因：

1. 当前最重要的是 Project A/MySQL 和 Project B/MySQL 的稳定隔离。
2. Traceway 已有 Rust 后端和桌面壳，原生 Rust FUSE 更容易复用本地配置、策略、缓存、审计和协作状态。
3. 数据库挂载需要严格的只读、脱敏、lookup、租户过滤和审计策略，应该由 Traceway 自己控制。
4. 减少额外 runtime、打包、升级和 API 稳定性风险。

后续多数据源扩展仍通过 Traceway 自己的 Connector Layer 完成：

1. MVP：原生 Rust MySQL connector。
2. Phase 3：抽象 Connector Layer。
3. 后续：按需增加 Postgres、Redis、S3、日志目录等 connector。

## 18. 平台注意事项

### 18.1 macOS

macOS 需要 macFUSE。桌面产品安装体验会受到系统扩展授权影响。

MVP 可先要求用户手动安装 macFUSE；产品化阶段再做安装检测、引导和错误诊断。

### 18.2 Linux

Linux FUSE 支持相对直接，但需要确认用户属于 `fuse` 相关权限组，或系统允许用户态挂载。

### 18.3 Windows

Windows 需要 WinFsp 或类似方案。MVP 可以先不支持 Windows FUSE，或者先用“materialized snapshot”模式生成普通文件作为替代。

## 19. MVP 范围

MVP 建议只做：

1. 单机本地使用。
2. Rust + `fuser` 只读 FUSE。
3. MySQL connector。
4. 手动输入 DSN。
5. 每个项目可创建多个 MySQL mount。
6. 每个 mount 独立凭证、策略、缓存、状态。
7. 输出 `README.md`、`connection.json`、`schema.sql`、`columns.json`、`indexes.sql`、`foreign_keys.json`、`inferred_relations.json`、`lookup_manifest.json`、`sample.jsonl`。
8. 支持主键 lookup、唯一索引 lookup、普通索引 lookup，全部通过地址型虚拟文件触发。
9. 支持有限 stats 文件，例如 `status_counts.json`、`null_counts.json`。
10. 默认 sample 行数限制 100。
11. 默认 lookup 行数限制 100。
12. 默认脱敏敏感字段。
13. UI 展示 mount status 和错误。

不做：

1. 写数据库。
2. 自定义 SQL 文件执行。
3. 任意 SQL。
4. 任意 JOIN。
5. 枚举整表行数据。
6. 自动连接生产库。
7. 多人共享 mount。
8. Windows 支持。

## 20. 后续阶段

| 阶段 | 目标 | 主要工作 |
| --- | --- | --- |
| Phase 1 | MySQL 只读 MVP | Rust FUSE、手动 DSN、schema/sample/lookup 文件树 |
| Phase 2 | 产品化 | Project Detector、Credential Vault、健康检查、缓存、审计 |
| Phase 3 | 多数据源 | Postgres、Redis、S3、日志目录、统一 Connector Layer |
| Phase 4 | 协作增强 | 脱敏 schema 共享、协作分析引用 mount evidence、团队策略 |

## 21. 风险与待确认问题

1. macFUSE 安装和授权会影响桌面交付体验。
2. FUSE 回调同步模型和异步 DB 查询需要谨慎处理，避免卡死文件系统请求。
3. `sample.jsonl` 读取如果设计不好，可能对数据库造成压力。
4. `count.txt` 对大表可能很慢，MVP 可返回估算或允许关闭。
5. 脱敏规则不能保证覆盖所有业务敏感字段，需要支持项目级配置。
6. Project Detector 从 `.env` 发现 DSN 时必须避免把 secrets 写入日志。
7. 挂载目录必须默认加入 `.gitignore`，避免被提交。
8. Codex sandbox 对项目外 mountpoint 的访问权限需要实测。
9. 地址型 lookup 如果过于宽松，可能被 Codex 反复读取造成数据库压力。
10. 没有真实 foreign key 的项目只能依赖推断关系，可能误导 Codex，必须标明置信度。
11. 多租户项目如果没有强制 tenant filter，lookup 可能读到其他租户数据。
12. `stats/top_values` 对高基数字段或敏感字段可能泄露数据分布，需要严格限制。

## 22. 验收标准

1. Project A 和 Project B 可以同时拥有同名 `mysql-main` 挂载，读取时分别命中各自 MySQL。
2. 挂载目录下可以通过 `ls`、`cat`、`find` 正常访问虚拟文件。
3. 所有写操作返回只读错误。
4. `schema.sql`、`columns.json`、`indexes.sql`、`foreign_keys.json`、`lookup_manifest.json`、`sample.jsonl` 内容正确。
5. `sample.jsonl` 不超过策略限制的行数和字节数。
6. 主键、唯一索引、普通索引 lookup 可以通过地址型虚拟文件直接读取。
7. 地址型 lookup 不会在 `readdir` 中枚举所有可能值。
8. MVP 不允许任意 SQL 和任意 JOIN。
9. 敏感字段默认脱敏。
10. 停止挂载后 mountpoint 不再可读或返回明确错误。
11. 后端重启后可以根据 registry 恢复 mount 状态，或明确显示需要手动重启。
12. UI 能展示每个 mount 的健康状态和最近错误。
13. 访问日志能记录虚拟文件读取结果。
