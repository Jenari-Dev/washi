#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""A made-up desktop notification server for the screenshot check (tests/screens.sh r6).

It owns org.freedesktop.Notifications on the session bus it is started on, answers `Notify` and
`GetServerInformation`, and prints what it was asked to show. Needs python3-dbus and PyGObject.
"""
import sys

import dbus
import dbus.mainloop.glib
import dbus.service
from gi.repository import GLib

NAME = "org.freedesktop.Notifications"


class Server(dbus.service.Object):
    def __init__(self, bus):
        super().__init__(bus, "/org/freedesktop/Notifications")
        self.last = 0

    @dbus.service.method(NAME, in_signature="susssasa{sv}i", out_signature="u")
    def Notify(self, app_name, replaces_id, icon, summary, body, actions, hints, timeout):
        self.last = int(replaces_id) or self.last + 1
        print(f"{app_name}: {summary} | {body}", flush=True)
        return self.last

    @dbus.service.method(NAME, out_signature="ssss")
    def GetServerInformation(self):
        return ("Sample notifications", "Washi tests", "1", "1.2")

    @dbus.service.method(NAME, out_signature="as")
    def GetCapabilities(self):
        return ["body"]


def main():
    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus()
    name = dbus.service.BusName(NAME, bus, do_not_queue=True)
    Server(bus)
    print("ready", flush=True)
    try:
        GLib.MainLoop().run()
    except KeyboardInterrupt:
        pass
    del name


if __name__ == "__main__":
    sys.exit(main())
