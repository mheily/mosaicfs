---
name: admin-crud
description: CRUD UX pattern for the MosaicFS admin UI. Use this when adding any new resource type to the admin — nodes, backends, VFS directories, credentials, etc. Defines the page structure, navigation flow, flash message handling, and Rust code conventions.
user-invocable: true
---

# Admin UI CRUD Pattern

This skill captures the settled UX and implementation pattern for CRUD in the
MosaicFS admin UI (`/admin/*`). Follow it exactly when adding new resource types
so the UI stays consistent.

---

## Page structure

Every resource gets **three pages**, not one or two:

### 1. List page — `GET /admin/{resource}`

- Heading + a single **"Create a {resource}"** button in the top-right, linked
  to the create page. No inline create form.
- Table of existing items. **Clicking anywhere on a row** navigates to the
  detail/edit page for that item. Use `cursor:pointer` to signal rows are
  clickable.
- **No delete button in the list.** The user must navigate to the detail page
  to delete.
- Shows flash messages from prior POST redirects.

```html
<table>
  <thead><tr><th>Name</th><th>...</th></tr></thead>
  <tbody>
    {% for r in items %}
    <tr onclick="location.href='/admin/resources/{{ r.id }}'" style="cursor:pointer">
      <td><code>{{ r.name }}</code></td>
      <td>...</td>
    </tr>
    {% endfor %}
  </tbody>
</table>
```

Note: drop the `<a>` tag inside the cell — the whole row is already the link.

### 2. Create page — `GET /admin/{resource}/new`

- Back link to the list page.
- Single focused form — one thing to fill out, nothing else on the page.
- `autofocus` on the first input so the user can start typing immediately.
- Plain-English labels and `<small>` help text on each field.
- Advanced/optional fields behind `<details>` with sensible server-side defaults
  so most users never need to open it.
- **Submit button + Cancel link** (Cancel goes back to the list).
- On success: redirect to the **list page** (`/admin/{resource}`) and flash a
  success message. Do not redirect to the new item's detail page — the user
  just created it and is back in context.
- On error: redirect back to this create page (`/admin/{resource}/new`) and
  flash the error. Do not send the user to the list page on failure.

```html
<p><a href="/admin/resources">← Resources</a></p>
<h2>Create a resource</h2>
<article>
  <form method="post" action="/admin/resources/create">
    <label>
      Name
      <input type="text" name="name" required autofocus>
      <small>Help text explaining what this field means.</small>
    </label>
    <details>
      <summary>Advanced options</summary>
      <!-- optional fields with good defaults -->
    </details>
    <div style="display:flex;gap:1rem;align-items:center">
      <button type="submit">Create resource</button>
      <a href="/admin/resources" class="secondary">Cancel</a>
    </div>
  </form>
</article>
```

### 3. Detail/edit page — `GET /admin/{resource}/{id}` (or `?id=` / `?path=`)

- Back link to the list page.
- Read-first: show what the item *is* before presenting edit controls. Use a
  summary line or card header with the key facts, then put edit forms below.
- Settings form for editable fields (POST to `/admin/{resource}/{id}/settings`).
- **Delete button at the bottom of the settings form**, separated by `<hr>`,
  with a `confirm()` guard. Use `class="secondary outline"` not `class="contrast"`
  to avoid making it the most prominent element on the page.
- All write actions redirect back to this detail page on success/failure, except
  delete which redirects to the list page.

```html
<p><a href="/admin/resources">← Resources</a></p>
<h2>{{ item.name }}</h2>

<article>
  <header><strong>Settings</strong></header>
  <form method="post" action="/admin/resources/{{ item.id }}/settings">
    <!-- editable fields -->
    <button type="submit">Save</button>
  </form>
  <hr>
  <form method="post" action="/admin/resources/{{ item.id }}/delete"
        onsubmit="return confirm('Delete {{ item.name }}?');" style="margin:0">
    <button type="submit" class="secondary outline">Delete this resource</button>
  </form>
</article>
```

---

## URL conventions

| Action | Method | URL |
|--------|--------|-----|
| List | GET | `/admin/{resource}` |
| Create form | GET | `/admin/{resource}/new` |
| Create submit | POST | `/admin/{resource}/create` |
| Detail/edit | GET | `/admin/{resource}/{id}` |
| Update settings | POST | `/admin/{resource}/{id}/settings` |
| Delete | POST | `/admin/{resource}/{id}/delete` |
| Sub-resource add | POST | `/admin/{resource}/{id}/{sub}/add` |
| Sub-resource delete | POST | `/admin/{resource}/{id}/{sub}/delete` |

If the resource ID contains slashes (e.g. virtual filesystem paths), pass it as
a form field (`<input type="hidden" name="id" value="...">`) rather than as a
URL path segment. Use a query param for GET pages (`?path=/foo/bar`). Use
`urlencoding::encode` when constructing redirect URLs that contain the ID.

---

## Rust implementation conventions

### File layout

- `src/admin/views.rs` — all GET handlers (read-only, render templates)
- `src/admin/actions.rs` — all POST handlers (writes, then redirect)
- `src/admin/mod.rs` — route registration + `tera()` template list
- `templates/*.html` — Tera templates (extend `layout.html`)

### POST-Redirect-GET pattern

Every write action follows the same structure:

```rust
pub async fn create_thing_action(
    State(state): State<Arc<AppState>>,
    session: Session,
    Form(form): Form<CreateThingForm>,
) -> Response {
    // 1. Validate input; on error flash and redirect back to the form page
    if form.name.trim().is_empty() {
        set_flash(&session, "Name is required.").await;
        return redirect("/admin/things/new");
    }

    // 2. Write to CouchDB directly (not through the REST API)
    match state.db.put_document(&doc_id, &doc).await {
        Ok(_) => {
            set_flash(&session, format!("'{}' created.", name)).await;
            redirect("/admin/things")          // success → list page
        }
        Err(e) => {
            set_flash(&session, format!("Create failed: {e}")).await;
            redirect("/admin/things/new")      // error → back to form
        }
    }
}
```

Key rules:
- Write handlers live in `actions.rs` and call CouchDB directly — do **not**
  call the REST API handlers (those return JSON, not redirects).
- Always redirect after POST (never render HTML from a POST handler).
- Success → redirect to the list or detail page, depending on action.
- Failure → redirect back to the page where the user was, so the flash appears
  in context.

### Flash messages

Flash messages are set with `set_flash(&session, msg).await` and consumed by
`page_ctx` on the next GET. They render as a blue banner at the top of every
page via `layout.html`.

The banner style uses explicit hex values (Pico's numbered CSS variables are
not available in the embedded Pico build):
```css
.flash {
  background: #dbeafe;
  border-left: 4px solid #2563eb;
  color: #1e3a8a;
  padding: 0.75rem 1rem;
  border-radius: 4px;
  margin-bottom: 1rem;
}
```

### Template registration

Every new template must be added to the `tera()` `OnceLock` in `mod.rs`:

```rust
("thing.html", include_str!("../../templates/thing.html")),
("thing_new.html", include_str!("../../templates/thing_new.html")),
```

And the routes registered in the `protected` router in `router()`:

```rust
.route("/admin/things", get(views::things_page))
.route("/admin/things/new", get(views::thing_new_page))
.route("/admin/things/create", post(actions::create_thing_action))
.route("/admin/things/:id", get(views::thing_detail_page))
.route("/admin/things/:id/settings", post(actions::patch_thing_action))
.route("/admin/things/:id/delete", post(actions::delete_thing_action))
```

---

## Checklist when adding a new resource

- [ ] List page: heading + create button, clickable rows (`cursor:pointer`, `onclick`), no delete button
- [ ] Create page: back link, focused form, autofocus, help text, advanced in `<details>`, submit + cancel
- [ ] Detail page: back link, read-first summary, settings form, delete at bottom with confirm guard
- [ ] Actions: PRG pattern, success → list, error → form, direct CouchDB writes
- [ ] Templates registered in `tera()` in `mod.rs`
- [ ] Routes registered in `protected` router in `mod.rs`
- [ ] Path normalization for user-supplied paths (prepend `/` if missing, strip trailing `/`)

$ARGUMENTS
