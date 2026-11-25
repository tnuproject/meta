//! Rendering.

use meta_common::{CODENAME, PROJECT, TAGLINE, VERSION};
use meta_eds_db::{Database, Validation};
use meta_eds_detect::Inventory;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

use crate::app::App;
use crate::catalog::desktops;
use crate::plan::Plan;
use crate::screens::Screen;

pub fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(2),
    ])
    .areas(frame.area());
    draw_header(frame, header, app);
    let [side, main] =
        Layout::horizontal([Constraint::Length(28), Constraint::Min(20)]).areas(body);
    draw_steps(frame, side, app);
    draw_screen(frame, main, app);
    draw_footer(frame, footer, app);
    if app.confirm_quit {
        draw_quit(frame);
    }
}

fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let title = format!(" {PROJECT} {VERSION} \"{CODENAME}\" installer: plan only ");
    let step = format!("Step {} of {}", app.screen_index + 1, Screen::ALL.len());
    let header = Paragraph::new(step).block(Block::new().borders(Borders::ALL).title(title));
    frame.render_widget(header, area);
}

fn draw_steps(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let items = Screen::ALL.iter().enumerate().map(|(index, screen)| {
        let current = index == app.screen_index;
        let marker = if current { ">" } else { " " };
        let style = if current { bold() } else { Style::new() };
        ListItem::new(format!("{marker} {:2} {}", index + 1, screen.title())).style(style)
    });
    frame.render_widget(
        List::new(items).block(Block::new().borders(Borders::ALL)),
        area,
    );
}

/// Draws the current screen. Takes the app mutably to keep the scroll position
/// within the text, whose height depends on the terminal.
fn draw_screen(frame: &mut Frame<'_>, area: Rect, app: &mut App) {
    let screen = app.screen();
    let block = Block::new()
        .borders(Borders::ALL)
        .title(format!(" {} ", screen.title()));
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (index, field) in screen.fields().iter().enumerate() {
        let selected = index == app.field_index;
        let value = field.value(&app.config);
        let value = if field.is_text() {
            format!("[{value}{}]", if selected { "_" } else { "" })
        } else {
            format!("< {value} >")
        };
        let style = if selected {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new()
        };
        lines.push(Line::from(vec![
            Span::raw(format!("{:24} ", field.label())),
            Span::styled(value, style),
        ]));
    }
    if !lines.is_empty() {
        lines.push(Line::default());
    }
    lines.extend(details(app));
    let width = area.width.saturating_sub(2).max(1) as usize;
    let visible = area.height.saturating_sub(2) as usize;
    let lines = wrap(lines, width);
    let hidden = lines.len().saturating_sub(visible);
    app.scroll = app.scroll.min(hidden);
    let more = app.scroll < hidden;
    let block = if more {
        block.title_bottom(" Down for more ")
    } else {
        block
    };
    let offset = u16::try_from(app.scroll).unwrap_or(u16::MAX);
    frame.render_widget(Paragraph::new(lines).block(block).scroll((offset, 0)), area);
}

/// Breaks long single-style lines so the number of lines is known for scrolling.
fn wrap(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    let mut wrapped = Vec::new();
    for line in lines {
        if line.width() <= width || line.spans.len() != 1 {
            wrapped.push(line);
            continue;
        }
        let style = line.style;
        let text = line.to_string();
        let mut current = String::new();
        for word in text.split(' ') {
            if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > width {
                wrapped.push(Line::styled(std::mem::take(&mut current), style));
                current.push_str("    ");
            } else if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        wrapped.push(Line::styled(current, style));
    }
    wrapped
}

fn details(app: &App) -> Vec<Line<'static>> {
    let text = |s: &str| Line::from(s.to_owned());
    match app.screen() {
        Screen::Welcome => vec![
            text(TAGLINE),
            Line::default(),
            text("This prototype collects choices and saves an installation plan."),
            text("It never changes a disk."),
        ],
        Screen::Disk => {
            let mut lines = vec![text("Detected disks:")];
            lines.extend(
                app.disks
                    .iter()
                    .map(|disk| Line::from(format!("  {}", disk.summary()))),
            );
            if app.disks.is_empty() {
                let reason = app
                    .disk_notice
                    .clone()
                    .unwrap_or_else(|| "none found".into());
                lines.push(Line::from(format!("  {reason}")));
            }
            lines.push(Line::default());
            lines.push(text("No disk is selected until you choose one."));
            lines
        }
        Screen::Edition => vec![
            text("Both editions share the FreeBSD kernel and the META base system."),
            text("Debian or Arch packages live in their own root under /compat/meta."),
        ],
        Screen::User => vec![
            text("sudo is always installed. The root password is locked."),
            text("Passwords are never written to the plan."),
        ],
        Screen::Desktop => desktops()
            .into_iter()
            .find(|desktop| app.config.desktop.as_deref() == Some(desktop.id.as_str()))
            .map(|desktop| {
                let mut lines = vec![
                    Line::from(format!("Display: {}", desktop.display)),
                    Line::from(format!("Packages: {}", desktop.packages.join(" "))),
                ];
                if !desktop.availability.available {
                    lines.push(Line::from(format!(
                        "Not available from the repository on {}.",
                        desktop.availability.date
                    )));
                }
                lines.extend(desktop.notes.map(Line::from));
                lines
            })
            .unwrap_or_default(),
        Screen::Drivers => driver_details(app.inventory.as_ref()),
        Screen::Review => review(app),
        _ => Vec::new(),
    }
}

fn driver_details(inventory: Option<&Inventory>) -> Vec<Line<'static>> {
    let Some(inventory) = inventory else {
        return vec![Line::from("Hardware detection requires FreeBSD.")];
    };
    let database = Database::builtin();
    let mut lines = vec![Line::from("Detected hardware with known drivers:")];
    for device in &inventory.devices {
        if let Some(entry) = database.lookup(device).first() {
            let tested = match entry.validation {
                Validation::Validated => "tested",
                Validation::Documented => "not tested by META",
            };
            lines.push(Line::from(format!("  {} ({tested})", entry.name)));
        }
    }
    lines
}

fn review(app: &App) -> Vec<Line<'static>> {
    match Plan::build(&app.config) {
        Err(problems) => {
            let mut lines = vec![Line::styled("Fix these problems first:", bold())];
            lines.extend(
                problems
                    .into_iter()
                    .map(|problem| Line::from(format!("  {problem}"))),
            );
            lines
        }
        Ok(plan) => {
            let mut lines: Vec<Line<'static>> = plan
                .steps
                .iter()
                .enumerate()
                .map(|(index, step)| {
                    let mark = if step.destructive {
                        " [ERASES DATA]"
                    } else {
                        ""
                    };
                    Line::from(format!("{:2}. {}{mark}", index + 1, step.description))
                })
                .collect();
            lines.push(Line::default());
            lines.extend(
                plan.warnings
                    .into_iter()
                    .map(|w| Line::from(format!("! {w}"))),
            );
            lines.push(Line::default());
            lines.push(Line::styled("Press Enter to save this plan.", bold()));
            lines
        }
    }
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let help =
        "Up/Down field  Left/Right change  Enter next  PgUp/PgDn screen  Esc quit".to_owned();
    let text = app.message.clone().unwrap_or(help);
    frame.render_widget(Paragraph::new(text), area);
}

fn draw_quit(frame: &mut Frame<'_>) {
    let area = frame.area();
    let width = 40.min(area.width);
    let height = 3.min(area.height);
    let area = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, area);
    let question = Paragraph::new("Quit without saving a plan? (y/n)")
        .block(Block::new().borders(Borders::ALL));
    frame.render_widget(question, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disks::Disk;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().to_string()
    }

    fn app() -> App {
        let disk = Disk {
            name: "ada0".into(),
            size_bytes: 64 << 30,
            description: "Example".into(),
            in_use: true,
        };
        App::new(vec![disk], None, None)
    }

    #[test]
    fn every_screen_renders_on_a_standard_console() {
        let mut app = app();
        for (index, screen) in Screen::ALL.iter().enumerate() {
            app.screen_index = index;
            let text = render(&mut app, 80, 25);
            assert!(text.contains(screen.title()), "{screen:?}\n{text}");
            assert!(text.contains("plan only"));
        }
    }

    #[test]
    fn tiny_terminals_do_not_panic() {
        let mut app = app();
        app.confirm_quit = true;
        for (width, height) in [(1, 1), (10, 3), (30, 8), (200, 60)] {
            for index in 0..Screen::ALL.len() {
                app.screen_index = index;
                render(&mut app, width, height);
            }
        }
    }

    #[test]
    fn disk_screen_shows_disks_and_use() {
        let mut app = app();
        app.screen_index = Screen::ALL.iter().position(|s| *s == Screen::Disk).unwrap();
        let text = render(&mut app, 100, 25);
        assert!(text.contains("ada0  64 GiB  Example, in use"), "{text}");
    }

    #[test]
    fn passwords_are_masked() {
        let mut app = app();
        app.screen_index = Screen::ALL.iter().position(|s| *s == Screen::User).unwrap();
        for c in "hunter2".chars() {
            app.config.user.password.push(c);
        }
        let text = render(&mut app, 100, 25);
        assert!(!text.contains("hunter2") && text.contains("*******"));
    }

    #[test]
    fn review_lists_problems_or_steps() {
        let mut app = app();
        app.screen_index = Screen::ALL.len() - 1;
        assert!(render(&mut app, 120, 40).contains("no disk selected"));
        app.config.disk = Some("ada0".into());
        app.config.user.name = "alice".into();
        assert!(render(&mut app, 120, 40).contains("[ERASES DATA]"));
    }

    #[test]
    fn long_review_scrolls_to_the_end_on_a_small_console() {
        let mut app = app();
        app.screen_index = Screen::ALL.len() - 1;
        app.config.disk = Some("ada0".into());
        app.config.user.name = "alice".into();
        app.config.desktop = Some("kde".into());
        let first = render(&mut app, 80, 25);
        assert!(
            first.contains("Down for more") && !first.contains("Press Enter"),
            "{first}"
        );
        for _ in 0..100 {
            app.scroll += 1;
            render(&mut app, 80, 25);
        }
        let last = render(&mut app, 80, 25);
        assert!(
            last.contains("Press Enter") && !last.contains("Down for more"),
            "{last}"
        );
        app.scroll = 0;
        assert_eq!(render(&mut app, 80, 25), first);
    }
}
