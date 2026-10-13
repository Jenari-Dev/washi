#pragma once
#include <QtCore/QString>

// Save the first top-level Qt Quick window (popups included) to a PNG; used by the headless
// screenshot check (WASHI_SCREENSHOT). Returns false when there is no window or saving fails.
bool washi_grab_window(const QString &path);
