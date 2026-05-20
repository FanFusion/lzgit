# Goal: comview-style Diff Rendering Parity

## 0. One-line Goal

把 `comview` 的 diff 渲染设计、视觉层次、语法高亮、行内 diff、高质量左右/上下视图完整还原到本项目，并顺手重构当前 diff 渲染代码，避免继续堆在超长文件里。

参考项目：`https://github.com/rockorager/comview`

本地参考副本：`/tmp/comview`

当前参考提交：`02e62ca2e59b60c88f2ffa838b8cd994d0af8f67`

License：MIT。若移植较大段算法、结构或测试思路，保留必要 attribution。

---

## Execution Gate

在用户明确说“开始”之前，只允许做 goal/方案/验收标准/测试计划层面的修改和验证，不允许改实现代码。

允许：

- 编辑 `GOAL.md`
- 补充设计清单
- 补充测试计划
- 补充端到端 tmux 验证方案
- 做只读调研和只读代码浏览
- 和用户对齐范围/顺序/验收标准

不允许：

- 修改 `src/` 实现代码
- 新增 Rust 模块
- 修改 Cargo 配置
- 改 UI 行为
- 运行会改变项目状态的实现脚本
- 开始重构

开始条件：用户明确回复类似 “开始”、“按这个做”、“执行”、“开始实现”。

如果需要概念验证，必须先单独说明 POC 范围，并等用户确认。

---

## 1. Product Direction

本项目不是要变成 comview，也不是把 comview 的 Go TUI 直接塞进来。

要做的是：

- 保留本项目的 Git/History 产品结构
- 保留本项目左侧导航 + 右侧 diff 的工作流
- 保留本项目已有的左右 diff view 和上下/unified diff view
- 把右侧 diff 的渲染品质升级到 comview 水平
- 把重复、臃肿的 diff 渲染代码拆成清晰模块

最终用户感知应该是：

> 这个项目还是 lzgit/tui-explorer-rs，但 diff 的美术、行内变化、高亮层次、阅读舒服程度接近 comview。

---

## 2. Existing Views That Must Remain

### 2.1 Git Tab Overall Layout

现有大结构必须保留：

```text
┌──────────── Git tree / status / files ────────────┬──────────── Diff view ────────────┐
│ staged / working / untracked / conflicts          │ side-by-side or unified diff      │
└───────────────────────────────────────────────────┴──────────────────────────────────┘
```

不能把 Git tab 改成 comview 那种纯 diff viewer。

### 2.2 History / Log Overall Layout

现有大结构必须保留：

```text
┌──────────── commit / reflog / stash list ─────────┬──────────── Diff view ────────────┐
│ history, reflog, stash, command output            │ side-by-side or unified diff      │
└───────────────────────────────────────────────────┴──────────────────────────────────┘
```

Files mode 也必须保留：

```text
┌──────── commit list ────────┬──── changed files ────┬──────── diff for file ────────┐
└─────────────────────────────┴───────────────────────┴──────────────────────────────┘
```

### 2.3 Diff 内部的两个 view 都要保留

本项目有两种 diff 内部布局，都必须升级：

#### A. 左右 view / side-by-side

```text
old line number  - old code      │ new line number  + new code
old line number    context       │ new line number    context
               empty             │ new line number  + added code
old line number  - deleted code  │ empty
```

要求：

- old/delete 永远左侧
- new/add 永远右侧
- context 两侧同时显示
- 空侧保留 gutter/code 宽度，列不塌陷
- separator 永远稳定
- wrap / scroll 不破坏左右对齐

#### B. 上下 view / unified

```diff
diff --git a/file b/file
@@ -10,7 +10,7 @@ fn demo() {
 context
-old code
+new code
 context
```

要求：

- 这就是用户说的“上下 view”
- 不是备选/降级视图，而是一等公民
- 视觉风格必须和左右 view 同源
- 语法高亮、行内高亮、主题色都要覆盖
- `s` 切换 side-by-side / unified 后体验一致

---

## 3. Areas To Modify

### 3.1 Git Working Diff

当前入口：

- `src/ui/tabs/git.rs`
- `render_diff_view`
- `render_side_by_side_diff`
- `render_unified_diff`
- `render_revert_buttons`
- 数据状态：`app.git.diff_lines`, `app.git.diff_mode`, `app.git.diff_scroll_x/y`

必须升级：

- 左右 diff
- 上下/unified diff
- hunk header
- file header
- metadata lines
- syntax highlight
- inline highlight
- revert hunk button
- revert block button

### 3.2 History / Log Diff

当前入口：

- `src/ui/tabs/log.rs`
- `render_diff_content`
- `render_log_side_by_side_diff`
- `render_log_unified_diff`
- 数据状态：`app.log_ui.diff_lines`, `app.log_ui.diff_mode`, `app.log_ui.diff_scroll_x/y`

必须升级：

- 左右 diff
- 上下/unified diff
- commit header + diff 内容分层
- file header
- hunk header
- metadata lines
- syntax highlight
- inline highlight
- Files mode 单文件 diff
- reflog/stash/commands 下不该崩

### 3.3 Shared Diff Data / Algorithm

当前位置：

- `src/git.rs`
- `GitDiffCell`
- `GitDiffRow`
- `build_side_by_side_rows`
- width / clip / pad helpers

必须重构为共享算法，不要 Git tab 和 History tab 各自乱搞一套。

---

## 4. Refactor Requirement: Do Not Keep A Huge File

用户明确要求：不要继续一个很长的大文件。

### 4.1 Current Problem

现在 diff 相关逻辑分散且重复：

- `src/git.rs` 同时有 Git 状态、tree、diff model、diff parsing、side-by-side helper
- `src/ui/tabs/git.rs` 有大量 diff 渲染逻辑
- `src/ui/tabs/log.rs` 又复制了一套类似 diff 渲染逻辑
- 未来加 comview 级别行内 diff 后，如果继续写进去，会更难维护

### 4.2 Target Module Split

建议拆成这些模块：

```text
src/
├── diff/
│   ├── mod.rs              # public exports
│   ├── model.rs            # DiffDocument, DiffFile, DiffHunk, DiffRow, DiffCell, InlineSpan
│   ├── parse.rs            # unified diff parser / raw lines -> model
│   ├── inline.rs           # comview-style line pairing + token inline diff
│   ├── side_by_side.rs     # model -> side-by-side rows
│   ├── unified.rs          # model -> unified rows
│   └── width.rs            # unicode width, safe clipping, padding, tabs
├── ui/
│   ├── diff_render.rs      # shared ratatui renderer helpers for Git + History
│   └── tabs/
│       ├── git.rs          # Git tab layout + calls shared diff renderer
│       └── log.rs          # Log tab layout + calls shared diff renderer
```

如果一次性拆这么多风险太高，可以分两步：

1. 先建 `src/diff_render.rs` 或 `src/diff/`，把新逻辑放进去
2. 再逐步从 `src/git.rs`, `src/ui/tabs/git.rs`, `src/ui/tabs/log.rs` 挪出重复代码

### 4.3 Refactor Boundaries

本轮可以重构：

- diff model
- diff parser
- inline diff algorithm
- side-by-side row builder
- unified row builder
- ratatui diff render helpers
- Git/History 两边调用方式

本轮不要重构：

- Git operation execution
- branch UI
- commit drawer
- terminal tab
- explorer tab
- conflict resolution logic，除非只是为了共享颜色/helper

### 4.4 File Size Goal

目标不是机械限制行数，但方向是：

- `src/git.rs` 不再继续增长 diff 渲染算法
- `src/ui/tabs/git.rs` 只保留 Git tab 布局和 Git-specific 操作按钮
- `src/ui/tabs/log.rs` 只保留 Log tab 布局和 History-specific header/files/sidebar
- diff 渲染细节集中到共享 renderer

---

## 5. Comview Visual Design To Match

这里是“美术设计要完全贴合”的具体拆解。

### 5.1 Overall Visual Tone

要贴近 comview：

- 暗色背景下低饱和、柔和 diff 背景
- add/delete 不用刺眼纯红纯绿
- 变化区域有两级层次：整行轻背景 + 行内更亮背景
- gutter 比代码更 dim，不抢视线
- file/hunk header 有明确层级
- metadata 更 muted
- separator 不突兀
- 语法高亮颜色要盖在 diff 背景上仍清楚

### 5.2 Color Roles

需要在 theme palette 中明确这些角色：

- `diff_add_bg`：新增整行背景
- `diff_del_bg`：删除整行背景
- `diff_add_inline_bg`：新增行内变化背景，更亮
- `diff_del_inline_bg`：删除行内变化背景，更亮
- `diff_hunk_bg`：hunk header 背景
- `diff_add_fg`：`+` marker / add accent
- `diff_del_fg`：`-` marker / delete accent
- `diff_gutter_fg`：line numbers
- `diff_meta_fg`：metadata / no-newline / index lines，可复用 border/size muted 色
- `diff_file_fg`：file header，可复用 accent_primary
- `diff_separator_fg`：middle separator / file separator

### 5.3 Theme Coverage

每个主题都要有可读结果：

- Terminal
- Mocha
- Tokyo Night
- Gruvbox
- Nord
- Dracula

不能只在一个主题好看。

### 5.4 Background Fill Rules

每一行要填满可见宽度，不要只给文字本身上背景：

- delete left column 背景填满左列
- add right column 背景填满右列
- unified add/delete 背景填满整行内容区域
- 空侧如果代表 missing side，可保持 bg 或使用轻微 muted bg，但宽度必须稳定
- hunk header 背景填满 diff content width
- inline background 只覆盖 changed tokens，不覆盖整行

### 5.5 File Header

贴合 comview 的信息层次，同时保留本项目更好看的 filename-first：

```text
📄 filename.ext  path/to/dir/
```

要求：

- filename 用 accent + bold
- directory 用 muted
- 多文件之间有空行或 separator
- separator 用 muted 横线，不要太亮

### 5.6 Hunk Header

hunk header：

```text
@@ -10,7 +10,8 @@ fn name() {
```

要求：

- 整行 hunk 背景
- range 部分 accent/bold
- trailing context/function name muted 或 dim
- 如果实现成本高，至少做到整行 hunk bg + accent bold

### 5.7 Metadata Lines

这些行要显示但不能抢视线：

- `index ...`
- `--- a/file`
- `+++ b/file`
- `rename from`
- `rename to`
- `new file mode`
- `deleted file mode`
- `similarity index`
- `Binary files ... differ`
- `\ No newline at end of file`

要求：

- muted/accent_secondary
- 不参与语法高亮
- 不参与 add/delete 行内 diff

### 5.8 Diff Stat / Commit Header

如果 History diff 带 commit header 或 diff stat：

- commit hash / subject：醒目
- author/date：muted 或 secondary
- commit body：普通 fg
- trailers：label 和 value 分层
- diff stat bar：`+` 用 add color，`-` 用 delete color
- header 和 diff 内容之间有空行分隔

---

## 6. Syntax Highlighting Requirements

语法高亮不能被还原样式弄坏。

### 6.1 Existing Highlighter Must Stay

现有依赖：

- syntect
- two-face

现有能力必须保留：

- 按扩展名高亮
- unsupported extension fallback
- syntax highlight toggle
- highlight cache 或 render cache 不失效

### 6.2 Side-by-side Syntax State

左右视图中：

- old side 和 new side 应分别维护 highlighter/parser state
- context 行理论上两侧都显示，不能让 old side 消耗后 new side 状态错乱
- delete 行只进入 old side highlighter
- add 行只进入 new side highlighter
- hunk/file/meta 行要 reset 或按文件更新 highlighter

### 6.3 Unified Syntax State

上下/unified 视图中：

- `+` / `-` marker 不参与 syntax highlight
- code 部分参与 syntax highlight
- highlighter state 按 visible logical order 前进
- `---` / `+++` headers 不当成 delete/add 代码

### 6.4 Inline Highlight Overlay

顺序应该是：

1. 得到 code text
2. 做语法高亮，生成 foreground spans
3. 根据 add/delete/context 背景设置 base bg
4. 根据 inline spans 覆盖 changed token bg
5. padding 到列宽时使用 line bg，不使用 inline bg

要求：

- inline bg 不能清掉 syntax fg
- inline spans 不能切坏 UTF-8
- horizontal scroll 后 inline highlight 仍对应可见 code
- wrap 后 inline highlight 仍尽量正确

---

## 7. Inline Diff Algorithm Requirements

还原 comview 的核心算法，不要只做简单字符串 diff。

### 7.1 Delete/Add Block Pairing

对连续的 delete/add block：

```diff
-old one
-old two
+inserted
+new one
+new two
```

不能简单 old[0] 对 new[0]。

要做：

- 对所有 old/new 行计算 similarity score
- 使用 DP 找全局最佳配对
- 低于 threshold 的不配对
- 允许跳过插入/删除行
- 配对结果用于 inline spans 和 side-by-side 排列

### 7.2 Similarity

参考 comview：

- tokenize code
- 优先用 word tokens 做相似度
- fallback 到 punctuation/symbol tokens
- LCS 计算 common tokens
- 起始 token 相同可加 bonus
- threshold 防止乱配

### 7.3 Tokenization

Token 类型：

- word：字母、数字、下划线，以及适当 Unicode letter/digit
- punctuation：标点
- symbol：其他非空白符号
- whitespace：跳过，不作为 token

要求：

- byte offset 保留原始 code 中位置
- 支持 UTF-8
- 支持 tab，不 panic

### 7.4 Span Generation

对配对 old/new 行：

- 找共同 prefix tokens
- 找共同 suffix tokens
- 中间用 LCS
- 未匹配 token 生成 changed spans
- 如果一边有 changed spans 另一边没有，参考 counterpart spans 逻辑补对应高亮
- 小 gap 可以合并，避免一个字符一个背景块

---

## 8. Gutter Requirements

### 8.1 Side-by-side Gutter

左侧 gutter：

```text
123- 
123  
     
```

右侧 gutter：

```text
124+ 
124  
     
```

要求：

- line number right-aligned
- marker 单独颜色
- 空侧保留同宽空白
- gutter 背景/前景与 code 分离
- gutter 不参与 horizontal scroll
- gutter 不参与 syntax highlight

### 8.2 Unified Gutter / Prefix

unified 每行：

```text
+ code
- code
  code
```

要求：

- prefix marker 单独 span
- marker fg 使用 add/delete/gutter 色
- code 部分可语法高亮
- 整行背景填满

---

## 9. Wrapping / Scrolling Requirements

### 9.1 Horizontal Scroll

wrap off 时：

- side-by-side：代码区横向滚动，gutter 不滚
- unified：可沿用当前 Paragraph horizontal scroll，但不能切坏样式
- truncation arrow/indicator 可保留，但不应打乱列宽

### 9.2 Wrap

wrap on 时：

- side-by-side 每侧独立 wrap
- 续行 gutter 为空，但 code 起点对齐
- 背景填满整列
- inline highlight 在续行中尽量正确
- syntax highlight 不崩

### 9.3 Unicode Width

必须处理：

- 中文
- emoji
- tab
- combining / zero-width 尽量不 panic
- 不从 UTF-8 中间切 string

---

## 10. Existing Interactions That Must Survive

不能破坏：

- `s` 切换左右 / 上下 unified
- syntax highlight toggle
- wrap toggle
- Git diff vertical scroll
- Git diff horizontal scroll
- Log diff vertical scroll
- Log diff horizontal scroll
- Git hunk revert button
- Git side-by-side block revert button
- mouse wheel scroll
- Git tree selection触发 diff 更新
- History item selection触发 diff 更新
- History Files mode selection触发 diff 更新
- diff cache invalidation
- conflict view
- full file view

---

## 11. Concrete Implementation Plan

### Phase 1: Alignment / No-code checkpoint

- 写完本 goal 文件
- 和用户对齐以下问题：
  - comview 视觉是否要求 1:1，还是在当前主题体系下近似还原
  - 是否接受新增 `src/diff/` 多文件模块
  - 是否接受较大重构 Git/Log diff render 调用方式
  - 是否优先 side-by-side，再 unified，还是两者同时完成

### Phase 2: Diff Model Extraction

新增或重构：

```text
src/diff/model.rs
src/diff/parse.rs
src/diff/width.rs
```

目标：

- 统一表示 raw diff
- 区分 row kind
- 保存 file path / syntax filename
- 保存 old/new line number
- 保存 code/prefix/gutter/marker

### Phase 3: Inline Algorithm

新增：

```text
src/diff/inline.rs
```

实现：

- tokenization
- similarity
- best line pairing
- inline spans
- span merging
- Unicode-safe byte offsets

### Phase 4: Layout Row Builders

新增：

```text
src/diff/side_by_side.rs
src/diff/unified.rs
```

实现：

- structured rows -> side-by-side display rows
- structured rows -> unified display rows
- preserve mapping for hunk/block revert buttons

### Phase 5: Shared Ratatui Renderer

新增：

```text
src/ui/diff_render.rs
```

实现：

- render file header
- render hunk header
- render metadata
- render unified code line
- render side-by-side code line
- gutter spans
- syntax highlight overlay
- inline highlight overlay
- padding / fill background

### Phase 6: Wire Git Tab

修改：

- `src/ui/tabs/git.rs`
- `src/main.rs` cache key if needed
- `src/git.rs` only if state mapping still lives there

目标：

- Git tab 调 shared renderer
- revert hunk/block 位置仍正确
- current scrolling/wrap/toggle 行为保留

### Phase 7: Wire History Tab

修改：

- `src/ui/tabs/log.rs`

目标：

- History diff 调 shared renderer
- commit header / files sidebar 保留
- style 与 Git tab diff 一致

### Phase 8: Theme Palette

修改：

- `src/main.rs` theme palette 或拆到独立 theme module

增加：

- inline add/delete bg
- meta fg if needed
- separator fg if needed

### Phase 9: Tests

见下一节。

### Phase 10: Validation / Install

执行：

```bash
cargo fmt
cargo test
cargo clippy -- -D warnings
cargo build --release
mkdir -p ~/.local/bin
install -m 755 target/release/lzgit ~/.local/bin/lzgit
```

---

## 12. Test Plan

必须加测试，不能只靠肉眼。

### 12.1 Inline Algorithm Tests

- `inline_spans_highlight_changed_word`
  - old: `let color = "red";`
  - new: `let color = "blue";`
  - expected：只高亮 `red` / `blue`

- `inline_spans_keep_common_prefix_suffix`
  - old/new 有长公共前后缀
  - expected：公共 prefix/suffix 不高亮

- `inline_spans_do_not_pair_unrelated_lines`
  - old/new 完全不同
  - expected：不产生 misleading inline spans 或不配对

- `inline_spans_coalesce_small_gaps`
  - 多个接近 token 变化
  - expected：合并为更可读 span

- `inline_spans_unicode_safe`
  - 中文、emoji、多字节字符
  - expected：不 panic，不切坏 UTF-8

- `inline_spans_tab_safe`
  - 包含 tab
  - expected：span offset 合法，display 不 panic

### 12.2 Row Pairing Tests

- `pairs_similar_replacement_rows`
  - old: `foo := oldValue + 1`
  - new: `foo := newValue + 1`
  - expected：配对，有 inline spans

- `pairs_replacements_around_inserted_line`
  - old 两行，new 三行，中间插入
  - expected：相似行配对，插入行单独右侧显示

- `does_not_pair_unrelated_delete_add_blocks`
  - unrelated block
  - expected：不配对

- `preserves_line_numbers_in_side_by_side`
  - hunk old/new start 不同
  - expected：行号正确

### 12.3 Parser / Row Model Tests

- parse file header
- parse hunk header
- parse add/delete/context
- parse no-newline marker
- parse rename metadata
- parse binary diff metadata
- parse new/deleted file metadata
- parse commit header/body/trailer if History uses it

### 12.4 Rendering Helper Tests

- gutter line number right aligned
- marker color role chosen correctly
- empty side width stable
- inline overlay preserves syntax foreground
- inline overlay changes only background
- clipping does not break UTF-8
- padding fills expected width
- hunk header splits range and context if implemented

### 12.5 Integration-ish Tests

Using a sample diff, assert generated display model includes:

- file row
- hunk row
- delete/add paired row
- inline spans
- metadata rows muted classification
- side-by-side row count expected
- unified row count expected

### 12.6 Manual Smoke Tests

真实运行：

- Git tab modified file：左右 diff 行内高亮
- Git tab `s` unified：上下 view 风格正确
- History tab commit：左右 diff 行内高亮
- History tab `s` unified：上下 view 风格正确
- History Files mode：选择文件后 diff 正确
- syntax highlight on/off：不崩，颜色可读
- wrap on/off：左右列不漂移
- horizontal scroll：gutter 稳定
- revert hunk/block：按钮位置正确并能点
- 大 diff：滚动不卡或不明显退化

---


### 12.7 End-to-end Tmux Visual Tests

可以加端到端测试，而且应该加。目标是用真实 terminal/tmux 跑 `lzgit`，在固定窗口尺寸下捕获 diff 画面，检查左右/上下视图、颜色 ANSI、语法高亮、gutter、inline highlight 是否存在。

#### Why tmux E2E

单测能证明算法正确，但不能证明最终终端画面正确。tmux E2E 用来覆盖：

- ratatui 最终渲染出的布局
- side-by-side 左右列是否稳定
- unified 上下 view 是否稳定
- ANSI style 是否真的输出
- gutter 是否固定
- wrap/horizontal scroll 后画面是否还能读
- Git tab 和 History tab 是否都接入同一套 renderer

#### Test Harness Design

建议新增脚本：

```text
scripts/e2e_diff_tmux.sh
```

脚本做这些事：

1. 创建临时 git repo
2. 写入固定内容的测试文件，比如 `src/demo.rs`, `README.md`, `unicode.txt`
3. `git init` + initial commit
4. 修改文件制造 deterministic diff
5. build 当前二进制，优先用 `target/debug/lzgit` 或 `target/release/lzgit`
6. 创建固定尺寸 tmux session，例如 `120x36`
7. 在 tmux pane 中启动 `lzgit <temp-repo>`
8. 等待首屏稳定
9. 发送按键进入 Git tab / 选择文件 / 切换 side-by-side / unified / History
10. 用 `tmux capture-pane -ep` 捕获带 ANSI 的 pane 内容
11. 保存快照到 `target/e2e/`
12. 对快照做自动断言
13. 可选：生成 HTML/PNG 供人工看
14. 清理 tmux session 和临时 repo

#### Snapshot Outputs

保存两类快照：

```text
target/e2e/git-side-by-side.ansi
target/e2e/git-side-by-side.txt
target/e2e/git-unified.ansi
target/e2e/git-unified.txt
target/e2e/history-side-by-side.ansi
target/e2e/history-side-by-side.txt
target/e2e/history-unified.ansi
target/e2e/history-unified.txt
```

说明：

- `.ansi` 保留颜色 escape codes，用来检查样式是否存在
- `.txt` 去掉 ANSI，用来检查布局文字、行号、列分隔符
- 如果环境有转换工具，可以再输出 `.html` 或 `.png`

#### Actual Screenshot Options

tmux 本身不直接生成图片截图，所以分层处理：

默认必须有：

```bash
tmux capture-pane -ep -t <session>:<window>.<pane> > target/e2e/git-side-by-side.ansi
tmux capture-pane -p  -t <session>:<window>.<pane> > target/e2e/git-side-by-side.txt
```

可选图片截图：

- 如果环境有 `aha`：`.ansi -> .html`
- 如果环境有 `ansi2html`：`.ansi -> .html`
- 如果环境有 headless browser / playwright：`.html -> .png`
- 如果环境有 `vhs`：直接录制/截图 terminal
- 如果在真实桌面终端里运行，可以用终端自己的 screenshot

本项目 CI/CLI 环境默认以 `.ansi` + `.txt` 为准，图片作为人工 review artifact，不作为硬依赖。

#### Automated Assertions

对 `.txt` 做结构断言：

- Git side-by-side 中存在中间 separator：`│`
- Git side-by-side 中同时存在 old/new line numbers
- Git side-by-side 中出现 delete marker `-` 和 add marker `+`
- Git unified 中出现 `@@` hunk header
- Git unified 中出现 `-old` / `+new` 风格行
- History side-by-side 中存在 commit subject/header 和 diff separator
- History unified 中存在 commit header + hunk header
- file header 显示 filename + directory

对 `.ansi` 做样式断言：

- 存在 ANSI color escape：`\x1b[`
- add/delete 行有不同 style 序列
- hunk header 有 background style
- inline highlight 开启后 changed token 附近有额外 background style

#### Manual Visual Review

脚本结束后输出：

```text
E2E snapshots written:
  target/e2e/git-side-by-side.ansi
  target/e2e/git-unified.ansi
  target/e2e/history-side-by-side.ansi
  target/e2e/history-unified.ansi

To inspect:
  less -R target/e2e/git-side-by-side.ansi
```

如果生成了 HTML/PNG，也输出路径：

```text
target/e2e/git-side-by-side.html
target/e2e/git-side-by-side.png
```

#### Deterministic Demo Diff

E2E repo 至少包含这些 case：

- Rust 文件：测试 syntax highlight + inline word change
- Markdown 文件：测试普通文本 diff
- rename：测试 metadata
- new file：测试 pure add
- deleted file：测试 pure delete
- unicode 文件：测试中文/emoji/tab 不炸
- multi-line replacement：测试智能配对

示例 Rust diff：

```rust
-let color = "red";
+let color = "blue";

-let count = old_value + 1;
+let count = new_value + 1;
```

期望：

- side-by-side 中 `red`/`blue`, `old_value`/`new_value` 有行内高亮
- unified 中 add/delete 背景和 marker 正确
- syntax fg 在背景上可读

#### CI / Local Policy

- `cargo test` 跑纯单测，必须快且稳定
- `scripts/e2e_diff_tmux.sh` 作为本地/CI 可选 e2e，依赖 tmux
- 如果 CI 没 tmux，脚本应明确 skip，而不是假通过
- 本任务完成前，本地必须至少跑一次 tmux e2e 并保存 artifact

## 13. Acceptance Criteria

完成标准：

- Git tab 左右 diff 贴近 comview 视觉
- Git tab 上下/unified diff 贴近 comview 视觉
- History tab 左右 diff 贴近 comview 视觉
- History tab 上下/unified diff 贴近 comview 视觉
- 行内 diff 是 token-level，不是简单整行高亮
- 语法高亮保留并与 diff 背景正确叠加
- gutter/code 分层，行号和 marker 清楚
- file/hunk/meta/commit header 层次清楚
- 所有主题下可读
- 代码被拆分，不再继续堆长文件
- Git/History 尽量复用同一套 diff renderer
- 现有交互不坏
- 测试覆盖核心算法和关键 render helper
- 有 tmux 端到端快照测试，至少产出 Git/History 的左右和上下 diff 快照
- `cargo fmt` 通过
- `cargo test` 通过
- `cargo clippy -- -D warnings` 通过
- release binary 已安装到 `~/.local/bin/lzgit`

---

## 14. Non-goals

本轮不做：

- 不引入 Go runtime
- 不直接嵌入 comview TUI
- 不改项目整体 tab 架构
- 不改 Git 操作语义
- 不新增 comview 的 comments/review notes/text objects
- 不重写 explorer/terminal/commit drawer
- 不做主题选择器大改，除非为了 diff 色彩角色必须补字段

---

## 15. Risks

- inline span byte offset 与 Unicode display width 可能错位
- wrap + horizontal scroll + syntax highlight + inline highlight 同时开启时容易列宽不齐
- History 和 Git 目前渲染重复，重构要避免引入行为差异
- change block display row 依赖 side-by-side row 数，配对逻辑变化后要核对 revert button 位置
- 大 diff 的 inline algorithm 可能影响性能，需要只对 delete/add block 做并限制极端情况
- 视觉 1:1 comview 与本项目主题体系可能有冲突，需要优先保证可读性

---

## 16. Alignment Questions For User

开始实现前需要和用户确认：

1. 视觉目标：是否要求尽可能 1:1 贴合 comview，还是允许在当前主题体系下做“comview 风格”的等价实现？
2. 文件拆分：是否接受新增 `src/diff/` 多模块，并把 `src/git.rs` 中 diff 相关逻辑迁出？
3. 完成顺序：是否接受我先完成 side-by-side，再完成 unified；还是必须一次性两种 view 同时提交？
4. History header：commit header 是否也要按 comview 的 commit metadata/trailer 风格细化？
5. 图标：file header 继续保留当前 `📄 filename dir`，还是改成更接近 comview 的纯文本 header？

默认建议：

- 视觉尽量 1:1 贴合 comview，但颜色映射进入当前主题系统
- 接受新增 `src/diff/` 和 `src/ui/diff_render.rs`
- side-by-side 和 unified 在同一轮完成，但实现时先搭 side-by-side 基础
- History header 顺手细化
- 保留当前 filename-first file header，因为和本项目整体 UI 更一致
