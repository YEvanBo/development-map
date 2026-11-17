# Development Map — Windows x64 Agent 部署说明

作者 EvanBo · Apache-2.0。此包供 AI Agent 解压部署，没有 GUI、安装向导、常驻启动项或调度器。

## 先确认部署对象

仅用于 Windows x86_64（Intel/AMD 64 位），已在 Windows 11 x64 实机测试。不是 32 位 x86、Windows ARM64、Linux 或 macOS 包。

读取 `package.json`、校验 `SHA256SUMS`，有 OpenSSH 时再验签。公钥指纹应与可信发行页核对：`SHA256:GqMPJ+OSLz2ZTEMJTF9G/t4lvDAvJal8Dd7jszwkuh8`。不要仅因包内自带公钥就认定来源可信。

```powershell
cmd /d /c 'ssh-keygen -Y verify -f provenance\allowed_signers -I EvanBo -n file -s SHA256SUMS.sig < SHA256SUMS'
if ($LASTEXITCODE -ne 0) { throw 'Signature verification failed' }
Get-Content .\SHA256SUMS | ForEach-Object {
    $expected, $file = $_ -split '  ', 2
    if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) {
        throw "Hash mismatch: $file"
    }
}
```

直接在 PowerShell 执行这些只读命令，无需运行 .ps1 文件。验签通过 cmd.exe 处理输入重定向。签名是 SSHSIG 发行签名，不是 Windows Authenticode 发布者签名。不要更改 ExecutionPolicy 或关闭 Windows 安全防护；没有 OpenSSH 时先核对来自可信发行渠道的外层 ZIP 哈希，明确签名尚未验证。

## 放置文件与检查依赖

将包放到一个新的版本目录，例如 `%LOCALAPPDATA%\DevelopmentMap\v0.1.0-rc.4`。保留目录整体，使用 `bin\*.exe` 的绝对路径。无需安装 Rust、Cargo、Node、SQLite 服务器或 VC++ 运行库；SQLite 和 C 运行库已静态链接。

Git 仅在注册仓库、验证 Git 对象或追踪改动时需要；Python 3 仅用于 `Skill/scripts/save-pending.py` 和附带 smoke.py。先检查已有工具，缺少时使用官方发行版按用户授权安装。Python 的实际命令可能是 `python`、`python3` 或 `py -3`，不要把 Windows Store 占位别名当作可用 Python。包不含 Git/Python、模型、Codex、DSH、依赖缓存或用户数据库。

不要覆盖已有版本、用户配置、地图或凭据。数据库及 token 文件放在安装目录之外的用户私有目录；升级已有非空地图前先备份并按 `docs/PROTOCOL.md` 迁移。

## 配置项目与宿主

1. 识别用户指定的工程、地图 `project_id` 与任务 `node_id`。目录、Git 和宿主分组只提供线索，不能据此自动合并项目。没有对应绑定时，询问关联已有任务还是创建；不要用缺权限或撤销错误触发自动创建。
2. 有现成数据库与凭据时使用它们的绝对路径；不要打印 token、塞进 MCP 配置或提交 Git。没有时由可信操作员按 `docs/CLI.md` 和 `docs/PROTOCOL.md` 初始化最小项目与身份，不创建演示项目冒充真实任务，不伪造用户目标确认。只安装软件无需创建任何业务任务。
3. 将 `Skill` 复制到实际项目根的 `.agents/skills/development-map`。已存在时先比较内容；相同直接复用，不同保留备份后按本次授权更新。Python 快照助手已经包含 Windows 刷盘修复。
4. Codex：参考 `config/codex.toml.example` 合入选定的 `.codex/config.toml` 或用户配置，只修改本服务表，不覆盖其他服务器。示例使用 `/` 风格绝对路径，避免 TOML 的反斜杠转义错误。
5. DSH：参考 `config/dsh.patch.yml.example`，通过已安装宿主支持的 profile/patch 入口加载；不要把 YAML 直接写成另一种宿主的 JSON 配置。路径与身份都由本次授权确定。
6. 将明确的当前任务和地图入口交给 Agent，必要时在项目已有指令中加入“本任务先使用 development-map Skill 读取配置的地图，再按 takeover 状态恢复或询问目标”。仅安装 Skill/MCP 不保证宿主在每个新会话自动调用。

目标确认仍为 operator-attested：可信操作员核对真实用户回复后登记，user_ref 不是宿主自动认证。无有效确认时询问用户并保存未交付工作。非可信 Worker 需要 OS/沙箱隔离数据库、WAL、备份和操作员凭据；同一不受限用户能读写 SQLite 时不构成安全隔离。

## 验收并停止

先执行 `bin\map-mcp.exe --version`。在用户授权的测试目录执行 `python smoke.py --work <一个不存在的新目录>`，测试只使用新建的合成地图，不写实际工程。smoke.py 是独立协议客户端测试，不是 Codex/DSH 模型会话认证。

在实际宿主检查 initialize、tools/list（map_resume/map_project/map_submit）和有权限的 resume；确认 EOF 后进程退出，重新启动仍能读到原节点。已有任务的只读接手不应自动提交 checkpoint。完成安装与直接相关检查后交付，不顺带规划、修改或执行项目里的其他任务。

卸载时移除本服务的配置条目及本版本程序目录；仅删除确认属于本次安装且未被修改的 Skill。保留数据库、备份、token、pending 快照和用户工程。回滚通过恢复旧配置指向旧版本完成，不覆盖已有数据库。

## English

Agent-oriented Windows x64 bundle: inspect metadata, verify hashes/signature, extract into a new versioned directory, and merge the supplied MCP examples without replacing existing host settings. Copy the portable Skill into the explicitly selected project's `.agents/skills/development-map`. Database and credentials stay outside the package; project/task identities and real user confirmations must be provisioned explicitly. No GUI, background service, scheduler or automatic host attestation is installed. Git and Python are optional external tools for repository operations and pending-work snapshots respectively. Run the isolated smoke test, verify the actual host's MCP lifecycle, then deliver and stop. Preserve user data during upgrade, rollback and removal.
