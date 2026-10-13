pragma Singleton

import QtQuick

// The host-side image work (scan session, previews, small file helpers). `engine` is the
// Imaging object (C++/Rust), or a fake in the tests. Slow calls answer through its done(id, ok,
// body) signal; the helpers here turn that into callbacks with the parsed JSON.
QtObject {
    id: img

    property var engine: null

    property var _pending: ({})
    property var _early: ({})

    function parse(body) {
        try {
            return JSON.parse(body);
        } catch (e) {
            return null;
        }
    }

    // cb(ok, result) once the job `id` finishes; result is the parsed JSON ({message} on failure).
    function wait(id, cb) {
        if (id in _early) {
            const r = _early[id];
            delete _early[id];
            if (cb) {
                cb(r[0], r[1]);
            }
        } else if (cb) {
            _pending[id] = cb;
        }
        return id;
    }

    function _onDone(id, ok, body) {
        const v = parse(body) || ({ message: body });
        const cb = _pending[id];
        if (cb) {
            delete _pending[id];
            cb(ok, v);
        } else {
            _early[id] = [ok, v];
        }
    }

    function reset() {
        _pending = ({});
        _early = ({});
    }

    // ---- convenience wrappers -------------------------------------------------------------
    function addPage(path, dpi, mode, cb) {
        return wait(engine.scanAddPage(path, dpi, mode), cb);
    }

    // mode: "page" | "work" | "work-geometry"
    function render(index, maxEdge, mode, cb) {
        return wait(engine.scanRender(index, maxEdge, mode), cb);
    }

    function save(req, cb) {
        return wait(engine.scanSave(JSON.stringify(req)), cb);
    }

    function extra(req, cb) {
        return wait(engine.scanExtra(JSON.stringify(req)), cb);
    }

    function photoPreview(req, cb) {
        return wait(engine.photoPreview(JSON.stringify(req)), cb);
    }

    function discPreview(req, px, cb) {
        return wait(engine.discPreview(JSON.stringify(req), px), cb);
    }

    function pageInfo(index) {
        return parse(engine.scanPageInfo(index));
    }

    function editState(fnResult) {
        return parse(fnResult);
    }

    function tools() {
        return (engine ? parse(engine.tools()) : null) || ({ pdf: false, office: false, ocr: false });
    }

    function fileUrl(path) {
        return path ? "file://" + path : "";
    }

    function localPath(url) {
        const s = String(url);
        return s.indexOf("file://") === 0 ? decodeURIComponent(s.substring(7)) : s;
    }

    function baseName(path) {
        const s = String(path);
        return s.substring(s.lastIndexOf("/") + 1);
    }

    function dirName(path) {
        const s = String(path);
        const i = s.lastIndexOf("/");
        return i > 0 ? s.substring(0, i) : "/";
    }

    property Connections link: Connections {
        target: img.engine
        ignoreUnknownSignals: true
        function onDone(id, ok, body) {
            img._onDone(id, ok, body);
        }
    }
}
