#!/usr/bin/env python3
"""Regenerate the portable Finder layout (development-only: ds-store==1.3.2)."""
from pathlib import Path

from ds_store import DSStore


with DSStore.open(str(Path(__file__).with_name("dmg-layout.DS_Store")), "w+") as store:
    store["."]["vSrn"] = ("long", 1)
    store["."]["icvl"] = ("type", b"icnv")
    store["."]["bwsp"] = {
        "ShowSidebar": False,
        "ShowStatusBar": False,
        "ShowToolbar": False,
        "ShowPathbar": False,
        "ShowTabView": False,
        "ContainerShowSidebar": False,
        "WindowBounds": "{{200, 200}, {600, 360}}",
    }
    store["."]["icvp"] = {
        "viewOptionsVersion": 1,
        "arrangeBy": "none",
        "gridOffsetX": 0.0,
        "gridOffsetY": 0.0,
        "gridSpacing": 100.0,
        "iconSize": 96.0,
        "textSize": 13.0,
        "labelOnBottom": True,
        "showItemInfo": False,
        "showIconPreview": False,
        "backgroundType": 0,
        "backgroundColorRed": 1.0,
        "backgroundColorGreen": 1.0,
        "backgroundColorBlue": 1.0,
        "scrollPositionX": 0.0,
        "scrollPositionY": 0.0,
    }
    store["Tidegauge.app"]["Iloc"] = (150, 160)
    store["Applications"]["Iloc"] = (450, 160)
