#!/usr/bin/env python3
"""Validate UI field keys and generated appearance presets against the backend schema."""
import json
import pathlib
import re

root = pathlib.Path(__file__).resolve().parents[1]
config = json.loads((root / "config.json").read_text())
source = (root / "src/settings.js").read_text()
fields = set(re.findall(r"^\s*\['([A-Za-z][A-Za-z0-9]*)','[^']+','(?:select|range|toggle|text)'", source, re.M))
virtual = {"discOpacity", "discSize", "centerHole", "centerHoleColor"}
presets = {"transitionStyle", "transitionDuration", "transitionDelay",
           "sleeveTransitionStyle", "sleeveAlignment", "sleeveDuration", "sleeveDelayIn", "sleeveDelayOut"}
missing = (fields - virtual - presets) - config.keys()
for suffix in ("Basic", "Cd", "Vinyl", "VinylCover", "CdCover", "AlbumCover"):
    missing.update(f"{k}{suffix}" for k in ("transitionStyle", "transitionDuration", "transitionDelay") if f"{k}{suffix}" not in config)
for suffix in ("VinylCover", "CdCover"):
    missing.update(f"{k}{suffix}" for k in presets if k.startswith("sleeve") and f"{k}{suffix}" not in config)
for key in ("discSizeBasic", "discSizeCd", "discSizeVinyl", "discSizeVinylCover", "discSizeCdCover", "discSizeAlbumCover",
            "discOpacityBasic", "discOpacityCd", "discOpacityVinyl", "discOpacityVinylCover", "discOpacityCdCoverUp", "discOpacityCdCoverDown", "discOpacityAlbumCover",
            "centerHoleBasic", "centerHoleCd", "centerHoleVinyl",
            "centerHoleColorBasic", "centerHoleColorCd", "centerHoleColorVinyl"):
    if key not in config:
        missing.add(key)
if missing:
    raise SystemExit("Settings UI keys missing from config.json: " + ", ".join(sorted(missing)))
print("Settings UI keys match config schema")
