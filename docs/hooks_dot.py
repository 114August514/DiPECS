"""MkDocs hook: register the Graphviz dot custom fence formatter for pymdownx.superfences."""

from __future__ import annotations

import subprocess
from typing import Any


def dot_formatter(
    source: str = "",
    language: str = "",
    css_class: str = "",
    options: dict[str, Any] | None = None,
    md: Any = None,
    classes: Any = None,
    id_value: str = "",
    attrs: dict[str, str] | None = None,
    **kwargs: Any,
) -> str | None:
    """Render DOT source as inline SVG via the system `dot` executable."""
    if language != "dot":
        return None

    try:
        result = subprocess.run(
            ["dot", "-Tsvg"],
            input=source,
            capture_output=True,
            text=True,
            check=True,
            timeout=10,
        )
        svg = result.stdout
        if svg.startswith("<?xml"):
            svg = svg.split("?>", 1)[1].strip()
        return f'<div class="dot-diagram">{svg}</div>'
    except (subprocess.CalledProcessError, FileNotFoundError, subprocess.TimeoutExpired):
        escaped = source.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
        return (
            f'<div class="admonition warning"><p class="admonition-title">'
            f"Graphviz rendering failed — showing raw DOT source</p>"
            f"<pre><code class=\"language-dot\">{escaped}</code></pre></div>"
        )


def on_config(config: Any, **kwargs: Any) -> None:
    """Register the dot formatter into pymdownx.superfences at config time."""
    for i, entry in enumerate(config.markdown_extensions):
        if isinstance(entry, dict):
            # Some plugins store extensions as dicts
            name = entry.get("pymdownx.superfences", "")
            if not name:
                continue
            ext_config = entry
        elif isinstance(entry, tuple):
            name = entry[0]
            ext_config = dict(entry[1]) if len(entry) > 1 else {}
        else:
            name = entry
            ext_config = {}
        if name == "pymdownx.superfences":
            fences = ext_config.setdefault("custom_fences", [])
            fences.append(
                {
                    "name": "dot",
                    "class": "dot-diagram",
                    "format": _dot_formatter,
                }
            )
            config.markdown_extensions[i] = (name, ext_config)
            break
