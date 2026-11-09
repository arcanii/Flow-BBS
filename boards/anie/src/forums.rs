//! The forums: each read and posted in from a role up, their threads a page
//! at a time with what's new marked, a thread's posts as one long text, and
//! what staff do to keep them tidy (lock, pin, remove a post or a thread,
//! move a thread). What a member wrote is shown through `printable`.

use std::io;

use flow_bbs::printable;

use crate::db::{Db, Role};
use crate::session::Session;
use crate::ui::{self, BOLD, CYAN, DIM, PLAIN, YELLOW};

/// A forum, as the list shows it.
struct Forum {
    id: i64,
    name: String,
    post_role: Role,
}

/// A thread, as reading it needs.
struct Thread {
    id: i64,
    forum: Forum,
    title: String,
    locked: bool,
    pinned: bool,
    last_post_id: i64,
}

/// What a typed command was: a number, or a letter.
pub enum Pick {
    Number(usize),
    Letter(char),
    Nothing,
}

/// Reads what was typed at a list's prompt.
pub fn pick(typed: &str) -> Pick {
    let t = typed.trim();
    if let Ok(n) = t.trim_start_matches('#').parse() {
        return Pick::Number(n);
    }
    let mut chars = t.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Pick::Letter(c.to_ascii_lowercase()),
        _ => Pick::Nothing,
    }
}

impl Session<'_> {
    /// The forums the member may read, and what's new in each.
    pub fn forums(&mut self) -> io::Result<()> {
        loop {
            self.doing("In the forums");
            let me = self.me().clone();
            let rows = self
                .db
                .query(
                    "SELECT f.id, f.name, f.description, f.post_role, \
                     (SELECT count(*) FROM threads t WHERE t.forum_id = f.id AND NOT t.deleted), \
                     (SELECT count(*) FROM threads t LEFT JOIN thread_reads r ON r.thread_id = t.id AND r.member_id = $1 \
                      WHERE t.forum_id = f.id AND NOT t.deleted AND (r.last_post_id IS NULL OR r.last_post_id < t.last_post_id)) \
                     FROM forums f WHERE NOT f.archived AND f.read_role = ANY($2) ORDER BY f.position, f.name",
                    &[&me.id, &me.role.and_below()],
                )
                .db()?;
            ui::heading(self.call, "Forums")?;
            let (cols, _) = ui::window(self.call);
            for (i, r) in rows.iter().enumerate() {
                let (name, description, threads, new): (String, String, i64, i64) = (r.get(1), r.get(2), r.get(4), r.get(5));
                let new = if new > 0 { format!("{YELLOW}{new} new{PLAIN}") } else { String::new() };
                self.call.say(&format!(" {CYAN}{:>2}{PLAIN} {BOLD}{}{PLAIN} {DIM}{threads:>4} threads{PLAIN}  {new}\n", i + 1, ui::pad(&name, 24)))?;
                self.call.say(&format!("    {DIM}{}{PLAIN}\n", ui::cut(&description, cols.saturating_sub(6))))?;
            }
            let Some(typed) = ui::answer(self.call, "\n Number to read, Q to go back: ", 4)? else { return Ok(()) };
            match pick(&typed) {
                Pick::Number(n) if (1..=rows.len()).contains(&n) => {
                    let r = &rows[n - 1];
                    let forum = Forum { id: r.get(0), name: r.get(1), post_role: Role::parse(r.get(3)).unwrap_or(Role::Sysop) };
                    self.forum(forum)?;
                }
                Pick::Letter('q') => return Ok(()),
                _ => {}
            }
            if self.call.hung_up() {
                return Ok(());
            }
        }
    }

    /// A forum's threads, a page at a time, pinned ones first.
    fn forum(&mut self, forum: Forum) -> io::Result<()> {
        let mut page = 0i64;
        loop {
            let me = self.me().clone();
            let (cols, rows) = ui::window(self.call);
            let per = rows.saturating_sub(6).max(5) as i64;
            let total: i64 = self.db.query_one("SELECT count(*) FROM threads WHERE forum_id = $1 AND NOT deleted", &[&forum.id]).db()?.get(0);
            let pages = ((total + per - 1) / per).max(1);
            page = page.clamp(0, pages - 1);
            let threads = self
                .db
                .query(
                    "SELECT t.id, t.title, m.handle, t.posts, to_char(t.last_post_at, 'MM-DD HH24:MI'), t.pinned, t.locked, \
                     (r.last_post_id IS NULL OR r.last_post_id < t.last_post_id) \
                     FROM threads t JOIN members m ON m.id = t.author_id LEFT JOIN thread_reads r ON r.thread_id = t.id AND r.member_id = $2 \
                     WHERE t.forum_id = $1 AND NOT t.deleted ORDER BY t.pinned DESC, t.last_post_at DESC LIMIT $3 OFFSET $4",
                    &[&forum.id, &me.id, &per, &(page * per)],
                )
                .db()?;
            ui::heading(self.call, &format!("{} (page {} of {pages})", forum.name, page + 1))?;
            if threads.is_empty() {
                self.call.say(&format!(" {DIM}No threads yet.{PLAIN}\n"))?;
            }
            let title_width = cols.saturating_sub(36).max(10);
            for (i, t) in threads.iter().enumerate() {
                let (title, author, posts, last, pinned, locked, new): (String, String, i32, String, bool, bool, bool) = (t.get(1), t.get(2), t.get(3), t.get(4), t.get(5), t.get(6), t.get(7));
                let marks = format!("{}{}{}", if new { format!("{YELLOW}*{PLAIN}") } else { " ".into() }, if pinned { "P" } else { " " }, if locked { "L" } else { " " });
                self.call.say(&format!(" {CYAN}{:>3}{PLAIN} {marks} {} {} {posts:>4}  {DIM}{last}{PLAIN}\n", i + 1, ui::pad(&title, title_width), ui::pad(&author, 12)))?;
            }
            let may_post = me.role >= forum.post_role;
            let mut prompt = String::from("\n Number to read");
            if may_post {
                prompt.push_str(", N for a new thread");
            }
            if pages > 1 {
                prompt.push_str(", ] next page, [ last page");
            }
            prompt.push_str(", Q to go back: ");
            let Some(typed) = ui::answer(self.call, &prompt, 4)? else { return Ok(()) };
            match pick(&typed) {
                Pick::Number(n) if (1..=threads.len()).contains(&n) => self.thread(threads[n - 1].get(0))?,
                Pick::Letter('n') if may_post => self.new_thread(&forum)?,
                Pick::Letter(']') => page += 1,
                Pick::Letter('[') => page -= 1,
                Pick::Letter('q') => return Ok(()),
                _ => {}
            }
            if self.call.hung_up() {
                return Ok(());
            }
        }
    }

    /// Reads thread `id`, if the member may: `None` if not.
    fn load_thread(&mut self, id: i64) -> io::Result<Option<Thread>> {
        let roles = self.me().role.and_below();
        let r = self
            .db
            .query_opt(
                "SELECT t.id, f.id, f.name, f.post_role, t.title, t.locked, t.pinned, t.last_post_id FROM threads t JOIN forums f ON f.id = t.forum_id \
                 WHERE t.id = $1 AND NOT t.deleted AND f.read_role = ANY($2)",
                &[&id, &roles],
            )
            .db()?;
        Ok(r.map(|r| Thread {
            id: r.get(0),
            forum: Forum { id: r.get(1), name: r.get(2), post_role: Role::parse(r.get(3)).unwrap_or(Role::Sysop) },
            title: r.get(4),
            locked: r.get(5),
            pinned: r.get(6),
            last_post_id: r.get(7),
        }))
    }

    /// A thread's posts, then what can be done with it.
    fn thread(&mut self, id: i64) -> io::Result<()> {
        loop {
            self.doing("Reading the forums");
            let me = self.me().clone();
            let Some(thread) = self.load_thread(id)? else { return Ok(()) };
            let posts = self
                .db
                .query(
                    "SELECT p.id, m.handle, p.author_id, p.body, p.deleted, to_char(p.created_at, 'YYYY-MM-DD HH24:MI'), p.edited_at IS NOT NULL \
                     FROM posts p JOIN members m ON m.id = p.author_id WHERE p.thread_id = $1 ORDER BY p.id",
                    &[&id],
                )
                .db()?;
            let (cols, _) = ui::window(self.call);
            let mut lines = Vec::new();
            for (k, p) in posts.iter().enumerate() {
                let (handle, body, deleted, at, edited): (String, String, bool, String, bool) = (p.get(1), p.get(3), p.get(4), p.get(5), p.get(6));
                let edited = if edited { " (edited)" } else { "" };
                lines.push(format!("{CYAN}#{}{PLAIN} {BOLD}{handle}{PLAIN} {DIM}{at}{edited}{PLAIN}", k + 1));
                if deleted {
                    lines.push(format!("{DIM}(This post was removed.){PLAIN}"));
                } else {
                    lines.extend(ui::wrap(&body, cols.saturating_sub(4)));
                }
                lines.push(String::new());
            }
            let title = format!("{} › {}{}", thread.forum.name, printable(&thread.title), if thread.locked { " (locked)" } else { "" });
            if !ui::page(self.call, &title, &lines)? {
                return Ok(());
            }
            self.db
                .execute(
                    "INSERT INTO thread_reads (member_id, thread_id, last_post_id) VALUES ($1, $2, $3) \
                     ON CONFLICT (member_id, thread_id) DO UPDATE SET last_post_id = GREATEST(thread_reads.last_post_id, EXCLUDED.last_post_id)",
                    &[&me.id, &id, &thread.last_post_id],
                )
                .db()?;
            let may_post = me.role >= thread.forum.post_role && !thread.locked;
            let staff = self.is(Role::Staff);
            let mut prompt = String::from(" ");
            if may_post {
                prompt.push_str("R to reply, ");
            }
            prompt.push_str("E to edit a post of yours, ");
            if staff {
                prompt.push_str(&format!("\n {}, D to remove a post, M to move, X to remove the thread,\n ", if thread.locked { "L to unlock" } else { "L to lock" }));
                prompt.push_str(if thread.pinned { "P to unpin, " } else { "P to pin, " });
            }
            prompt.push_str("Enter to read again, Q to go back: ");
            let Some(typed) = ui::answer(self.call, &prompt, 4)? else { return Ok(()) };
            let post_number = |s: &mut Self, prompt: &str| -> io::Result<Option<usize>> {
                let Some(n) = ui::answer(s.call, prompt, 4)? else { return Ok(None) };
                Ok(match pick(&n) {
                    Pick::Number(n) if (1..=posts.len()).contains(&n) => Some(n - 1),
                    _ => None,
                })
            };
            match pick(&typed) {
                Pick::Letter('q') => return Ok(()),
                Pick::Letter('r') if may_post => self.reply(&thread)?,
                Pick::Letter('e') => {
                    if let Some(k) = post_number(self, " Which post (its #)? ")? {
                        let (post_id, author, body, deleted): (i64, i64, String, bool) = (posts[k].get(0), posts[k].get(2), posts[k].get(3), posts[k].get(4));
                        if author != me.id || deleted {
                            ui::oops(self.call, "Only a post of your own can be edited.")?;
                            self.pause()?;
                        } else if let Some(new) = ui::editor(self.call, &body)? {
                            self.db.execute("UPDATE posts SET body = $2, edited_at = now() WHERE id = $1 AND author_id = $3", &[&post_id, &new, &me.id]).db()?;
                        }
                    }
                }
                Pick::Letter('l') if staff => {
                    self.db.execute("UPDATE threads SET locked = NOT locked WHERE id = $1", &[&id]).db()?;
                    self.audit(None, if thread.locked { "thread unlocked" } else { "thread locked" }, &thread.title)?;
                }
                Pick::Letter('p') if staff => {
                    self.db.execute("UPDATE threads SET pinned = NOT pinned WHERE id = $1", &[&id]).db()?;
                    self.audit(None, if thread.pinned { "thread unpinned" } else { "thread pinned" }, &thread.title)?;
                }
                Pick::Letter('d') if staff => {
                    if let Some(k) = post_number(self, " Remove which post (its #)? ")? {
                        let post_id: i64 = posts[k].get(0);
                        self.db.execute("UPDATE posts SET deleted = true, deleted_by = $2 WHERE id = $1", &[&post_id, &me.id]).db()?;
                        let author: String = posts[k].get(1);
                        self.audit(None, "post removed", &format!("#{} by {author} in {}", k + 1, thread.title))?;
                    }
                }
                Pick::Letter('x') if staff => {
                    if ui::yes(self.call, " Remove this whole thread?")? == Some(true) {
                        self.db.execute("UPDATE threads SET deleted = true WHERE id = $1", &[&id]).db()?;
                        self.audit(None, "thread removed", &thread.title)?;
                        return Ok(());
                    }
                }
                Pick::Letter('m') if staff => self.move_thread(&thread)?,
                _ => {}
            }
            if self.call.hung_up() {
                return Ok(());
            }
        }
    }

    /// A new thread in `forum`: its title and first post.
    fn new_thread(&mut self, forum: &Forum) -> io::Result<()> {
        self.doing("Writing in the forums");
        ui::heading(self.call, &format!("{} › a new thread", forum.name))?;
        let title = loop {
            let Some(t) = ui::answer(self.call, " Title: ", 80)? else { return Ok(()) };
            if t.chars().count() >= 3 {
                break t;
            }
            ui::oops(self.call, "A title is at least 3 letters.")?;
        };
        let Some(body) = ui::editor(self.call, "")? else { return Ok(()) };
        let me = self.me().clone();
        let mut t = self.db.transaction().db()?;
        let thread: i64 = t.query_one("INSERT INTO threads (forum_id, title, author_id) VALUES ($1, $2, $3) RETURNING id", &[&forum.id, &title, &me.id]).db()?.get(0);
        let post: i64 = t.query_one("INSERT INTO posts (thread_id, author_id, body) VALUES ($1, $2, $3) RETURNING id", &[&thread, &me.id, &body]).db()?.get(0);
        t.execute("UPDATE threads SET posts = 1, last_post_id = $2, last_post_at = now() WHERE id = $1", &[&thread, &post]).db()?;
        t.execute("INSERT INTO thread_reads (member_id, thread_id, last_post_id) VALUES ($1, $2, $3)", &[&me.id, &thread, &post]).db()?;
        t.commit().db()?;
        ui::done(self.call, "Posted.")?;
        self.pause()?;
        Ok(())
    }

    /// A reply to `thread`.
    fn reply(&mut self, thread: &Thread) -> io::Result<()> {
        self.doing("Writing in the forums");
        self.call.say(&format!("\n {BOLD}Your reply to {}{PLAIN}\n", printable(&thread.title)))?;
        let Some(body) = ui::editor(self.call, "")? else { return Ok(()) };
        let me = self.me().clone();
        let mut t = self.db.transaction().db()?;
        // Locked meanwhile: the reply waits for staff to unlock it.
        let open = t.query_opt("SELECT 1 FROM threads WHERE id = $1 AND NOT locked AND NOT deleted FOR UPDATE", &[&thread.id]).db()?.is_some();
        if !open {
            drop(t);
            ui::oops(self.call, "That thread was locked or removed a moment ago.")?;
            self.pause()?;
            return Ok(());
        }
        let post: i64 = t.query_one("INSERT INTO posts (thread_id, author_id, body) VALUES ($1, $2, $3) RETURNING id", &[&thread.id, &me.id, &body]).db()?.get(0);
        t.execute("UPDATE threads SET posts = posts + 1, last_post_id = $2, last_post_at = now() WHERE id = $1", &[&thread.id, &post]).db()?;
        t.commit().db()?;
        Ok(())
    }

    /// Moves `thread` to another forum.
    fn move_thread(&mut self, thread: &Thread) -> io::Result<()> {
        let forums = self.db.query("SELECT id, name FROM forums WHERE NOT archived AND id <> $1 ORDER BY position, name", &[&thread.forum.id]).db()?;
        for (i, f) in forums.iter().enumerate() {
            self.call.say(&format!(" {CYAN}{:>2}{PLAIN} {}\n", i + 1, f.get::<_, String>(1)))?;
        }
        let Some(typed) = ui::answer(self.call, " Move it to which (its number)? ", 3)? else { return Ok(()) };
        if let Pick::Number(n) = pick(&typed) {
            if let Some(f) = forums.get(n.wrapping_sub(1)) {
                let to: i64 = f.get(0);
                self.db.execute("UPDATE threads SET forum_id = $2 WHERE id = $1", &[&thread.id, &to]).db()?;
                self.audit(None, "thread moved", &format!("{} to {}", thread.title, f.get::<_, String>(1)))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_is_a_number_or_one_letter() {
        assert!(matches!(pick(" 12 "), Pick::Number(12)));
        assert!(matches!(pick("#3"), Pick::Number(3)));
        assert!(matches!(pick("N"), Pick::Letter('n')));
        assert!(matches!(pick("]"), Pick::Letter(']')));
        assert!(matches!(pick("no"), Pick::Nothing));
        assert!(matches!(pick(""), Pick::Nothing));
    }
}
