# 用 Agent 安装 Development Map

本包包含 V1.1 任务范围管理、schema 14、五个 MCP 工具及同版本 Skill。把压缩包解压到长期保留的版本目录。先用独立可信的 EvanBo 指纹 `SHA256:GqMPJ+OSLz2ZTEMJTF9G/t4lvDAvJal8Dd7jszwkuh8` 核验外包 SSHSIG、SHA-256 和包内清单。核验命令见下文。

可以把这一段发给 Agent：

> 请从这个已验签的平台包安装开发地图到当前项目。先读取 DEPLOY.md，确定我用 Codex 还是 DSH，并找出我已指定的地图数据库及私有 token 文件路径。缺少绑定就问我。先运行 install.py plan，让我看到会改哪些文件。需要写受保护的配置时，用宿主的单次审批申请执行对应的 apply 命令。我点允许后继续安装，开新会话用 map_resume 读回分给我的节点、已确认目标和接手状态，交付后停止。

1. 在**目标机器**确认系统和 CPU。macOS ARM64 适用于 Apple Silicon，Linux ARM64 和 Linux x64 需要 glibc 2.36+ 和 libgcc_s，Windows x64 使用 ZIP。WSL 选 Linux 包。核心运行需要对应平台二进制。安装入口和自检使用 Python 3.9+，Git 用于仓库验证。
2. 选择已有项目目录、该项目获授权的数据库和 token 文件，数据库与凭据放在版本目录之外。Unix token 权限设为 0600。新项目先由可信操作员依据 docs/CLI.md 配置身份。把旧库投入使用前先备份，按 docs/PROTOCOL.md 由操作员迁移并验证；安装脚本只配置文件。授权安装与授权工程任务是两次不同的决定。
3. 在解压目录运行下列命令，用真实的绝对路径替换占位符。Windows 可用已安装的 `python` 或 `py -3`。其中 `plan` 只读，会列出具体路径、摘要、变更清单和 `plan_id`。Codex 配置合入项目的 .codex/config.toml；DSH 默认生成一个专用的 .dsh/development-map.patch.json，也可用 `--dsh-patch` 指定专用位置。两者都会安装项目 Skill 和简短工程入口。
   
   ```sh
   python3 install.py plan --host codex --project "/absolute/project" --db "/absolute/state/map.sqlite" --token-file "/absolute/private/worker.token"
   python3 install.py apply --host codex --project "/absolute/project" --db "/absolute/state/map.sqlite" --token-file "/absolute/private/worker.token" --plan-id "<plan_id>"
   ```

4. DSH 用相同参数将 `--host codex` 换为 `--host dsh`。Agent 请求单次执行 **plan 对应的 apply 命令**，由宿主展示权限卡。无交互审批通道时，把计划和精确命令交给用户执行；拒绝时停止。摘要用来防止批准后文件改变，不能代替宿主的权限授予。已有文件有用户改动时检查提示，重新 plan。脚本检测到安装锁时，先确认先前进程已退出和文件状态，再清理该项目锁并重新 plan。配置中只写 token 文件路径。
5. 在受信项目开新的 Codex 会话。DSH 使用 `dsh headless --patch "/absolute/project/.dsh/development-map.patch.json"` 或同一 patch 启动支持的 profile，模型须已登录。用真实工具列出 map_resume、map_project、map_submit、map_scope_check、map_scope_report，并以已分配 project_id 调用 map_resume。应读回已有任务与目标。读到 EXPIRED_OR_REVOKED 要请操作员核对，不应另建节点。宿主公用或全局配置不由这个项目入口修改。
6. 要先体验合成项目，在包内运行 `python3 smoke.py --work "/absolute/new-test-directory"`（目录不能已存在）。仅用于试用时，可明确选用新目录中的 map.sqlite 和 other-token.txt 以及项目 acceptance、节点 other。该测试凭据一小时到期。另可运行 `python3 test_install.py` 在临时工程核验安装流程。

## 签名、升级与回退

下载压缩包、同名 .sha256 和 .sig；用已信任的 allowed_signers 执行 `ssh-keygen -Y verify -f allowed_signers -I EvanBo -n file -s ARCHIVE.sig < ARCHIVE`，将 ARCHIVE 替换为实际文件名。Windows x64 实测可在 WSL 中使用同一条命令核验 ZIP。此次笔记本的系统 OpenSSH 在读取大 ZIP 时停滞；遇到相同情况，请在 WSL 或可信的 Linux/macOS 机器上完成外包验签，再在 Windows 上比对 SHA-256。解压后对 SHA256SUMS 与 SHA256SUMS.sig 执行同样的验签，用 `shasum -a 256 -c SHA256SUMS`（macOS）或 `sha256sum -c SHA256SUMS`（Linux）核对内容。Windows 可使用 Get-FileHash 对照清单。

保留上一版本、安装前项目配置副本和升级前数据库备份。更换二进制版本时分别指定新目录和原有数据库/token 位置。若回退已迁移的库，恢复到单独路径并使用原版本与对应备份；新版本上的新增事件应保留。人工自定义 Skill 和不兼容的旧配置需要先核对。未经信任的 Worker 仍需操作系统隔离。

## Agent installation

Verify the archive and signed manifest, extract to a stable version directory, then run `install.py plan` for the selected project, host, database and token-file paths. Review the paths and request **one host approval** for the matching `apply --plan-id` command. Start a fresh trusted host session and verify the assigned node with `map_resume`. DSH requires the generated per-project patch when launching its profile. See the numbered steps above for commands, fixture credentials and rollback.
