#include "shot.h"

#include <QGuiApplication>
#include <QImage>
#include <QQuickWindow>

bool washi_grab_window(const QString &path)
{
    for (QWindow *w : QGuiApplication::topLevelWindows()) {
        if (auto *q = qobject_cast<QQuickWindow *>(w)) {
            const QImage img = q->grabWindow();
            return !img.isNull() && img.save(path);
        }
    }
    return false;
}
