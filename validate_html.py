"""index.html 启动前校验脚本 -- 拦截常见 JS 语法陷阱。
用法: python validate_html.py
返回 0 = 通过, 1 = 有错误
"""
import re
import sys

ERRORS = []


def err(line, msg):
    ERRORS.append(f"  Line {line}: {msg}")


def check():
    with open("index.html", "r", encoding="utf-8") as f:
        lines = f.readlines()

    content = "".join(lines)

    # 开关: 若 ENABLE_HTML_VALIDATE = false 则跳过校验
    if re.search(r'ENABLE_HTML_VALIDATE\s*=\s*false', content, re.IGNORECASE):
        print("[SKIP] ENABLE_HTML_VALIDATE = false, 跳过校验")
        return

    # ── 1. Alpine 属性中的反引号模板字符串 ──
    for i, line in enumerate(lines, 1):
        matches = re.finditer(
            r'(@\w+|x-(?:init|effect|show|text|html|cloak|data|model|bind)|:[\w-]+)\s*=\s*"([^"]*)"',
            line,
        )
        for m in matches:
            attr_name, attr_value = m.group(1), m.group(2)
            if "`" in attr_value:
                err(i, f"Alpine 属性 {attr_name} 中包含反引号 ` (会导致 SyntaxError)")

    # ── 2. 多行 Alpine 属性括号配对 ──
    multiline_attrs = re.finditer(
        r'(@\w+|x-\w+|:[\w-]+)\s*=\s*"([^"]*)"',
        content,
        re.DOTALL,
    )
    for m in multiline_attrs:
        attr_value = m.group(2)
        opens = attr_value.count("(")
        closes = attr_value.count(")")
        if opens != closes:
            line_num = content[: m.start()].count("\n") + 1
            err(line_num, f"Alpine 属性括号不匹配: {opens} '(' vs {closes} ')'")

    # ── 3. <script> 块内的 JS 保留字未加引号 ──
    in_script = False
    for i, line in enumerate(lines, 1):
        # 先检查关闭标签，再检查开启标签（处理同行的情况）
        if "</script>" in line:
            in_script = False
            continue
        if "<script" in line:
            in_script = True
            continue
        if not in_script:
            continue
        stripped = line.strip()
        if stripped.startswith("//"):
            continue
        for kw in ("import", "export"):
            if re.search(rf"(?<=[,{{\s]){kw}\s*:", line):
                err(i, f"JS 保留字 '{kw}' 未加引号用作属性名")

    # ── 4. <script> 块内全角冒号/分号 (跳过注释和字符串) ──
    in_script = False
    for i, line in enumerate(lines, 1):
        if "</script>" in line:
            in_script = False
            continue
        if "<script" in line:
            in_script = True
            continue
        if not in_script:
            continue
        stripped = line.strip()
        if stripped.startswith("//") or stripped.startswith("/*"):
            continue
        if "'" in line or '"' in line or "`" in line:
            continue
        for j, c in enumerate(line):
            if c == "：":
                err(i, f"全角冒号 (col {j+1})")
            elif c == "；":
                err(i, f"全角分号 (col {j+1})")

    # ── 5. Box-drawing 字符在 <script> 块内 ──
    in_script = False
    skip_self = False
    for i, line in enumerate(lines, 1):
        if "</script>" in line:
            in_script = False
            skip_self = False
            continue
        if "<script" in line:
            in_script = True
            skip_self = False
            continue
        if not in_script:
            continue
        # 跳过内嵌校验脚本自身 (它包含 box-drawing 正则)
        if "启动前校验" in line or "拦截常见 JS 语法陷阱" in line or "pre-flight check" in line:
            skip_self = True
            continue
        if skip_self:
            continue
        # 跳过 JS 注释行（注释中的 Box-drawing 字符无害）
        stripped = line.strip()
        if stripped.startswith("//") or stripped.startswith("/*") or stripped.startswith("*"):
            continue
        for j, c in enumerate(line):
            cp = ord(c)
            if 0x2500 <= cp <= 0x257F or 0x2580 <= cp <= 0x259F:
                err(i, f"Box-drawing U+{cp:04X} (col {j+1})")

    # ── 6. _i18n 对象中的 import 保留字 ──
    i18n_start = content.find("_i18n: {")
    if i18n_start != -1:
        i18n_section = content[i18n_start : i18n_start + 60000]
        if re.search(r"(?<=,)\s*import\s*:", i18n_section) or re.search(
            r"(?<=\{)\s*import\s*:", i18n_section
        ):
            line_num = content[:i18n_start].count("\n") + 1
            err(line_num, "_i18n 对象中 'import' 未加引号")

    # ── 7. Alpine 回调中裸用 lang（.then/.map 等箭头函数内 with 作用域失效） ──
    alpine_pattern = re.compile(
        r'(@[\w-]+|x-(?:init|effect|show|text|html|cloak|data|model|bind)|:[\w-]+)\s*=\s*"([^"]*)"'
    )
    for m in alpine_pattern.finditer(content):
        attr_value = m.group(2)
        if re.search(r"\.(?:then|map|filter|forEach|reduce)\s*\(", attr_value):
            if re.search(r"(?<!this\.)(?<!\w)lang\b", attr_value):
                line_num = content[: m.start()].count("\n") + 1
                err(line_num, "Alpine 回调中裸用 lang（应改为 this.lang）")


if __name__ == "__main__":
    check()
    if ERRORS:
        print(f"[FAIL] 发现 {len(ERRORS)} 个问题:\n")
        for e in ERRORS:
            print(e)
        print("\n修复后重新运行: python validate_html.py")
        sys.exit(1)
    else:
        print("[OK] index.html 校验通过")
        sys.exit(0)
