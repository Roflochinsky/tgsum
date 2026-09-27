#!/usr/bin/env python3
"""Synthetic GTK window for the read-only AT-SPI transport test."""

import json
import os
import sys

import gi

gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, GLib


window = Gtk.Window(title="Private Customer Alice message 123")
window.set_name("session-private")
window.set_default_size(320, 120)
button = Gtk.Button(label="Another private label")
button.set_name("123456789")
window.add(button)
window.connect("destroy", Gtk.main_quit)
window.show_all()


def announce_ready():
    print(json.dumps({"pid": os.getpid(), "executable": os.readlink("/proc/self/exe")}))
    sys.stdout.flush()
    return GLib.SOURCE_REMOVE


GLib.idle_add(announce_ready)
Gtk.main()
