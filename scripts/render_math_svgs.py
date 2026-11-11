#!/usr/bin/env python3
"""
Extracts all ```math blocks from README.md (sections 1..7), renders each
via MathJax v3 (tex-svg, local fontCache -> pure vector SVG paths), normalizes
dimensions at a uniform 1em = 15pt scale and white #FFFFFF foreground for GPU
theme tinting, writes deterministic gzipped .svgz files into assets/math/, and
generates src/math_assets.rs.

Usage:
    python scripts/render_math_svgs.py
"""

from __future__ import annotations

import gzip
import html
import json
import os
import re
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
README_PATH = os.path.join(ROOT, "README.md")
ASSETS_DIR = os.path.join(ROOT, "assets", "math")
RUST_OUT = os.path.join(ROOT, "src", "math_assets.rs")

# MathJax uses 1000 SVG units per 1em. 18.0 logical UI pt per 1em gives crisp, prominent equations cleanly.
PT_PER_1000_UNITS = 18.0 / 1000.0


def find_browser() -> str:
    candidates = [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        os.path.expandvars(r"%LOCALAPPDATA%\Google\Chrome\Application\chrome.exe"),
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        shutil.which("google-chrome") or "",
        shutil.which("chromium") or "",
        shutil.which("chromium-browser") or "",
        shutil.which("msedge") or "",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    ]
    for path in candidates:
        if path and os.path.isfile(path):
            return path
    raise RuntimeError("Could not find Chrome/Edge binary for headless MathJax rendering.")


def parse_readme_math_blocks(readme_path: str) -> dict[int, list[str]]:
    with open(readme_path, "r", encoding="utf-8") as f:
        lines = f.read().splitlines()

    sections: dict[int, list[str]] = {i: [] for i in range(1, 8)}
    current_sec: int | None = None
    in_math = False
    buf: list[str] = []

    for line in lines:
        sec_m = re.match(r"^###\s+([1-7])\.\s+", line)
        if sec_m and not in_math:
            current_sec = int(sec_m.group(1))
            continue

        if re.match(r"^##\s+", line) and not line.startswith("### ") and not in_math:
            if current_sec == 7:
                current_sec = None
            continue

        stripped = line.strip()
        if current_sec is not None:
            if not in_math and stripped == "```math":
                in_math = True
                buf = []
            elif in_math and stripped == "```":
                in_math = False
                tex = "\n".join(buf).strip()
                if tex:
                    sections[current_sec].append(tex)
            elif in_math:
                buf.append(line)

    return sections


def render_svgs_with_mathjax(sections: dict[int, list[str]]) -> list[tuple[int, int, str, str]]:
    items: list[tuple[int, int, str]] = []
    for sec in range(1, 8):
        for idx, tex in enumerate(sections[sec], start=1):
            items.append((sec, idx, tex))

    eq_divs = "\n".join(
        f'<div class="mj-eq" data-sec="{sec}" data-idx="{idx}">$$\n{html.escape(tex)}\n$$</div>'
        for sec, idx, tex in items
    )

    page_html = f"""<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<script>
window.MathJax = {{
  tex: {{
    inlineMath: [['$', '$']],
    displayMath: [['$$', '$$']],
    processEscapes: true,
    processEnvironments: true
  }},
  svg: {{
    fontCache: 'local'
  }},
  startup: {{
    pageReady: () => MathJax.startup.defaultPageReady().then(() => {{
      const xs = new XMLSerializer();
      const out = [];
      for (const el of document.querySelectorAll('.mj-eq')) {{
        const svg = el.querySelector('svg');
        out.push({{
          sec: parseInt(el.dataset.sec, 10),
          idx: parseInt(el.dataset.idx, 10),
          svg: svg ? xs.serializeToString(svg) : ''
        }});
      }}
      const pre = document.createElement('pre');
      pre.id = 'svg-json';
      pre.textContent = JSON.stringify(out);
      document.body.appendChild(pre);
      document.body.setAttribute('data-ready', 'true');
    }})
  }}
}};
</script>
<script src="https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-svg.js"></script>
</head>
<body>
{eq_divs}
</body>
</html>
"""

    browser = find_browser()
    with tempfile.TemporaryDirectory() as tmpdir:
        html_file = os.path.join(tmpdir, "math.html")
        with open(html_file, "w", encoding="utf-8") as f:
            f.write(page_html)
        file_url = "file:///" + html_file.replace("\\", "/")
        res = subprocess.run(
            [
                browser,
                "--headless=new",
                "--disable-gpu",
                "--allow-file-access-from-files",
                "--virtual-time-budget=15000",
                "--dump-dom",
                file_url,
            ],
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=True,
        )
        dom = res.stdout

    if 'data-ready="true"' not in dom:
        raise RuntimeError("MathJax did not finish rendering before timeout.")
    if "<mjx-merror" in dom or "data-mjx-error" in dom:
        raise RuntimeError("MathJax reported a TeX syntax error in one of the README blocks.")

    m = re.search(r'<pre id="svg-json">(.*?)</pre>', dom, flags=re.DOTALL)
    if not m:
        raise RuntimeError("Failed to locate serialized SVG JSON payload in DOM.")
    payload = json.loads(html.unescape(m.group(1)))
    if len(payload) != len(items):
        raise RuntimeError(f"Expected {len(items)} SVGs, extracted {len(payload)}.")

    results: list[tuple[int, int, str, str]] = []
    for entry, (_, _, tex) in zip(payload, items):
        clean_svg = normalize_svg(entry["svg"])
        results.append((int(entry["sec"]), int(entry["idx"]), tex, clean_svg))
    return results


def strip_ns(tag: str) -> str:
    return tag.split("}", 1)[-1]


def normalize_svg(raw_svg: str) -> str:
    # Ensure XML namespaces exist so ElementTree parses cleanly
    if 'xmlns:xlink=' not in raw_svg:
        raw_svg = raw_svg.replace("<svg ", '<svg xmlns:xlink="http://www.w3.org/1999/xlink" ', 1)
    root = ET.fromstring(raw_svg)
    vb = root.attrib.get("viewBox", "")
    parts = [float(p) for p in vb.split()]
    if len(parts) != 4 or parts[2] <= 0 or parts[3] <= 0:
        raise RuntimeError(f"Invalid MathJax viewBox: {vb!r}")
    _, _, vb_w, vb_h = parts
    px_w = vb_w * PT_PER_1000_UNITS
    px_h = vb_h * PT_PER_1000_UNITS

    def clean_elem(el: ET.Element) -> None:
        tag = strip_ns(el.tag)
        el.tag = tag
        new_attrib: dict[str, str] = {}
        for k, v in el.attrib.items():
            attr_name = strip_ns(k)
            # Normalize xlink:href -> href (resvg/usvg supports both; plain href avoids ns clutter)
            if k.endswith("}href") or k == "xlink:href":
                attr_name = "href"
            if attr_name in ("role", "focusable", "aria-hidden", "style") or attr_name.startswith("data-"):
                continue
            v = v.replace("currentColor", "#FFFFFF")
            new_attrib[attr_name] = v
        el.attrib = new_attrib
        for child in list(el):
            clean_elem(child)

    clean_elem(root)
    root.attrib["xmlns"] = "http://www.w3.org/2000/svg"
    root.attrib["width"] = f"{px_w:.2f}"
    root.attrib["height"] = f"{px_h:.2f}"
    root.attrib["viewBox"] = vb
    # Root g uses stroke/fill #FFFFFF; ensure white root fallback as well
    return ET.tostring(root, encoding="unicode")


def main() -> None:
    sections = parse_readme_math_blocks(README_PATH)
    total = sum(len(v) for v in sections.values())
    print(f"Extracted {total} math blocks across {len(sections)} sections from README.md.")

    rendered = render_svgs_with_mathjax(sections)

    if os.path.isdir(ASSETS_DIR):
        for name in os.listdir(ASSETS_DIR):
            if name.endswith(".svg") or name.endswith(".svgz"):
                os.remove(os.path.join(ASSETS_DIR, name))
    os.makedirs(ASSETS_DIR, exist_ok=True)

    raw_bytes_total = 0
    gz_bytes_total = 0
    per_section: dict[int, list[tuple[str, str, float, float]]] = {i: [] for i in range(1, 8)}

    for sec, idx, _tex, svg_str in rendered:
        svg_bytes = svg_str.encode("utf-8")
        gz_bytes = gzip.compress(svg_bytes, compresslevel=9, mtime=0)
        raw_bytes_total += len(svg_bytes)
        gz_bytes_total += len(gz_bytes)

        fname = f"s{sec}_{idx:02d}.svgz"
        out_path = os.path.join(ASSETS_DIR, fname)
        with open(out_path, "wb") as f:
            f.write(gz_bytes)

        root = ET.fromstring(svg_str)
        w = float(root.attrib["width"])
        h = float(root.attrib["height"])
        per_section[sec].append((f"s{sec}_{idx:02d}", fname, w, h))

    rust_lines = [
        "// @generated by scripts/render_math_svgs.py from README.md — do not edit manually.",
        "",
        "pub struct MathSvgAsset {",
        "    pub id: &'static str,",
        "    pub width_pt: f32,",
        "    pub height_pt: f32,",
        "    pub svgz: &'static [u8],",
        "}",
        "",
    ]

    for sec in range(1, 8):
        rust_lines.append(f"pub const SECTION_{sec}: &[MathSvgAsset] = &[")
        for eq_id, fname, w, h in per_section[sec]:
            rust_lines.extend(
                [
                    "    MathSvgAsset {",
                    f'        id: "{eq_id}",',
                    f"        width_pt: {w:.2f}_f32,",
                    f"        height_pt: {h:.2f}_f32,",
                    f'        svgz: include_bytes!("../assets/math/{fname}"),',
                    "    },",
                ]
            )
        rust_lines.append("];")
        rust_lines.append("")

    with open(RUST_OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(rust_lines))

    print(
        f"Wrote {len(rendered)} .svgz assets into assets/math/ "
        f"(raw SVG: {raw_bytes_total / 1024:.1f} KB, compressed svgz: {gz_bytes_total / 1024:.1f} KB)."
    )


if __name__ == "__main__":
    main()
