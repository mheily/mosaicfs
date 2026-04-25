/* browse_open.js — file-open handler for the MosaicFS browse UI.
 * Fetches POST /ui/browse/open (JSON), then calls the Tauri open_file command.
 * No framework, no build step. */

(function () {
  'use strict';

  /* The most recent server-resolved target (OpenTarget), kept so that
   * the retry and authorize buttons can re-use it without re-fetching. */
  var _currentTarget = null;

  // ── HTML escaping ──────────────────────────────────────────────────────────

  function escapeHtml(s) {
    return String(s)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#39;');
  }

  // ── Flash slot ─────────────────────────────────────────────────────────────

  function setFlash(html) {
    var el = document.getElementById('flash');
    if (el) el.innerHTML = html;
  }

  function clearFlash() { setFlash(''); }

  function wrapFlash(inner) {
    return '<div class="flash">' + inner + '</div>';
  }

  // ── Flash rendering ────────────────────────────────────────────────────────

  function renderFlash(code, fields) {
    var f = fields || {};
    var msg;
    switch (code) {
      case 'no_host_mount':
        msg = 'No mount configured for node <code>' + escapeHtml(f.node_id || '') +
          '</code> on this host. Add a network mount in the admin UI.';
        setFlash(wrapFlash(msg));
        break;

      case 'bookmark_not_authorized':
        msg = 'MosaicFS needs permission to open files under <code>' +
          escapeHtml(f.local_mount_path || '') + '</code> (node <code>' +
          escapeHtml(f.node_id || '') + '</code>).';
        setFlash(wrapFlash(msg + ' <button onclick="browseAuthorize(' +
          "'" + escapeHtml(f.local_mount_path || '') + "'" +
          ')">Authorize…</button>'));
        break;

      case 'path_not_accessible':
        msg = 'File not reachable: <code>' + escapeHtml(f.relative_path || '') +
          '</code> is not at <code>' + escapeHtml(f.local_mount_path || '') +
          '</code>. The share may be disconnected.';
        setFlash(wrapFlash(msg + ' <button onclick="browseRetry()">Retry</button>'));
        break;

      case 'path_traversal':
        msg = 'Refusing to open <code>' + escapeHtml(f.requested_path || '') +
          '</code>: a symlink resolved to <code>' + escapeHtml(f.resolved_path || '') +
          '</code>, outside <code>' + escapeHtml(f.local_mount_path || '') + '</code>.';
        setFlash(wrapFlash(msg));
        break;

      case 'mismatched_selection':
        msg = 'You selected <code>' + escapeHtml(f.got || '') +
          '</code>, but MosaicFS needs permission for <code>' +
          escapeHtml(f.expected || '') + '</code>.';
        setFlash(wrapFlash(msg + ' <button onclick="browseAuthorize(' +
          "'" + escapeHtml(f.expected || '') + "'" +
          ')">Try again</button>'));
        break;

      case 'user_cancelled':
        clearFlash();
        break;

      case 'bookmark_creation_failed':
        setFlash(wrapFlash('Couldn’t save permission for that folder: ' +
          escapeHtml(f.message || '')));
        break;

      case 'not_found':
        setFlash(wrapFlash('File not found.'));
        break;

      default:
        setFlash(wrapFlash(escapeHtml(f.message || code)));
        break;
    }
  }

  // ── Tauri open_file ────────────────────────────────────────────────────────

  function doOpenFile(target) {
    return window.__TAURI__.core.invoke('open_file', { target: target })
      .then(function () { clearFlash(); })
      .catch(function (e) { renderFlash(e.code || 'open_failed', e); });
  }

  // ── Public: authorize a mount then retry open ──────────────────────────────

  function browseAuthorize(localMountPath) {
    if (!window.__TAURI__) return;
    window.__TAURI__.core.invoke('authorize_mount', { localMountPath: localMountPath })
      .then(function () {
        if (_currentTarget) return doOpenFile(_currentTarget);
      })
      .catch(function (e) { renderFlash(e.code || 'open_failed', e); });
  }

  // ── Public: retry open_file with the current target ────────────────────────

  function browseRetry() {
    if (!window.__TAURI__ || !_currentTarget) return;
    doOpenFile(_currentTarget);
  }

  // ── Public: open a file by virtual path ───────────────────────────────────

  function browseOpen(virtualPath) {
    fetch('/ui/browse/open', {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: 'path=' + encodeURIComponent(virtualPath),
    })
      .then(function (resp) {
        var ct = resp.headers.get('content-type') || '';
        if (!ct.includes('application/json')) {
          renderFlash('open_failed', { message: 'Unexpected response from server.' });
          return;
        }
        resp.json().then(function (data) {
          if (!resp.ok) {
            renderFlash(data.code || 'open_failed', data);
            return;
          }
          /* data is OpenTarget: { node_id, local_mount_path, relative_path } */
          _currentTarget = data;
          if (!window.__TAURI__) {
            setFlash(wrapFlash(
              'This file can only be opened from the MosaicFS desktop app.'
            ));
            return;
          }
          doOpenFile(data);
        });
      })
      .catch(function (err) {
        renderFlash('open_failed', { message: err.message || String(err) });
      });
  }

  // ── Click delegation ───────────────────────────────────────────────────────

  document.addEventListener('click', function (e) {
    var el = e.target.closest('[data-browse-open]');
    if (!el) return;
    var path = el.getAttribute('data-virtual-path');
    if (path) browseOpen(path);
  });

  // ── Exports ────────────────────────────────────────────────────────────────

  window.browseOpen = browseOpen;
  window.browseAuthorize = browseAuthorize;
  window.browseRetry = browseRetry;
  /* Exposed for the browser-based unit tests (browse_open.test.html). */
  window._browseRenderFlash = renderFlash;
  window._browseEscapeHtml = escapeHtml;
})();
