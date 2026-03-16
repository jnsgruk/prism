use scraper::{ElementRef, Html, Selector};

/// A person extracted from the Canonical staff directory HTML.
pub struct DirectoryPerson {
    pub display_name: String,
    pub email: String,
    pub github_username: String,
    pub launchpad_username: String,
    pub mattermost_username: Option<String>,
    pub group: Option<String>,
    pub title: Option<String>,
    /// Manager name extracted from the `--manager` meta list item.
    pub manager_name: Option<String>,
    /// Number of `<ol>` ancestors this entry is nested within (1 = top-level).
    pub depth: u32,
}

/// Count how many `<ol>` ancestors an element has, starting from the
/// first `<ol>` with class `p-list` (the directory root list).
fn ol_depth(el: &ElementRef<'_>) -> u32 {
    let mut depth: u32 = 0;
    let mut node = el.parent();
    while let Some(parent) = node {
        if let Some(element) = parent.value().as_element()
            && element.name() == "ol"
        {
            depth += 1;
        }
        node = parent.parent();
    }
    depth
}

/// Parse a Canonical staff directory HTML page and extract people.
///
/// Expects the standard directory layout with `.p-media-object` entries
/// containing name, email, GitHub, and Launchpad fields.
/// Entries missing any required field (name, email, GitHub, Launchpad) are skipped.
///
/// Also extracts the `<ol>` nesting depth and manager name for each person,
/// which enables building the team hierarchy from the reporting tree.
pub fn parse_directory_html(html: &str) -> Vec<DirectoryPerson> {
    let document = Html::parse_document(html);

    #[allow(clippy::unwrap_used)] // Static selectors — known valid at compile time
    let sel = Selectors::new();

    document
        .select(&sel.media_object)
        .filter_map(|el| {
            let name = el
                .select(&sel.title_link)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())?;

            let email = el
                .select(&sel.email_link)
                .next()
                .and_then(|e| e.value().attr("href"))
                .and_then(|href| href.strip_prefix("mailto:"))
                .map(str::to_string)?;

            let github = el
                .select(&sel.github_link)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .filter(|s| !s.is_empty())?;

            let launchpad = el
                .select(&sel.launchpad_link)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .filter(|s| !s.is_empty())?;

            let mattermost = el
                .select(&sel.mattermost_link)
                .next()
                .map(|e| e.text().collect::<String>().trim().to_string())
                .filter(|s| !s.is_empty());

            let group = el.select(&sel.group_link).find_map(|a| {
                a.value()
                    .attr("href")
                    .filter(|h| h.starts_with("/groups/"))
                    .map(|_| a.text().collect::<String>().trim().to_string())
            });

            let title = el
                .select(&sel.content_p)
                .next()
                .map(|p| p.text().collect::<String>().trim().to_string())
                .filter(|s| !s.is_empty());

            let manager_name = el
                .select(&sel.manager_link)
                .next()
                .map(|a| a.text().collect::<String>().trim().to_string());

            let depth = ol_depth(&el);

            Some(DirectoryPerson {
                display_name: name,
                email,
                github_username: github,
                launchpad_username: launchpad,
                mattermost_username: mattermost,
                group,
                title,
                manager_name,
                depth,
            })
        })
        .collect()
}

/// Pre-compiled CSS selectors for directory parsing.
struct Selectors {
    media_object: Selector,
    title_link: Selector,
    email_link: Selector,
    github_link: Selector,
    launchpad_link: Selector,
    mattermost_link: Selector,
    group_link: Selector,
    content_p: Selector,
    manager_link: Selector,
}

impl Selectors {
    #[allow(clippy::unwrap_used)]
    fn new() -> Self {
        Self {
            media_object: Selector::parse(".p-media-object").unwrap(),
            title_link: Selector::parse(".p-media-object__title > a").unwrap(),
            email_link: Selector::parse(
                ".p-media-object__meta-list-item--email a[href^=\"mailto:\"]",
            )
            .unwrap(),
            github_link: Selector::parse(".p-media-object__meta-list-item--github a").unwrap(),
            launchpad_link: Selector::parse(".p-media-object__meta-list-item--launchpad a")
                .unwrap(),
            mattermost_link: Selector::parse(".p-media-object__meta-list-item--mattermost a")
                .unwrap(),
            group_link: Selector::parse(".p-media-object__content a[href^=\"/groups/\"]").unwrap(),
            content_p: Selector::parse("p.p-media-object__content").unwrap(),
            manager_link: Selector::parse(
                ".p-media-object__meta-list-item--manager a[href^=\"/people/\"]",
            )
            .unwrap(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn sample_html() -> String {
        r#"<!doctype html>
<html>
<body>
<ol class="p-list">
  <li>
    <div class="p-media-object">
      <div class="p-media-object__details">
        <h2 class="p-media-object__title">
          <a href="/people/jnsgruk">Jon Seager</a>
          <span>(He/Him)</span>
        </h2>
        <p class="p-media-object__content">VP Engineering, Charm Engineering (VP)</p>
        <p class="p-media-object__content">
          <strong>Group:</strong>
          <a href="/groups/Charm Engineering">Charm Engineering</a>
        </p>
        <ul class="p-media-object__meta-list">
          <li class="p-media-object__meta-list-item--email">
            Email: <a href="mailto:jon.seager@canonical.com">jon.seager@canonical.com</a>
          </li>
          <li class="p-media-object__meta-list-item--mattermost">
            Mattermost: <a href="https://chat.canonical.com/canonical/messages/@jnsgruk">jnsgruk</a>
          </li>
          <li class="p-media-object__meta-list-item--launchpad">
            Launchpad: <a href="https://launchpad.net/~jnsgruk">jnsgruk</a>
          </li>
          <li class="p-media-object__meta-list-item--github">
            Github: <a href="https://github.com/jnsgruk">jnsgruk</a>
          </li>
          <li class="p-media-object__meta-list-item--manager">
            Manager: <a href="/people/Mark Shuttleworth">Mark Shuttleworth</a>
          </li>
        </ul>
      </div>
    </div>
  </li>
  <ol type="I">
    <li>
      <div class="p-media-object">
        <div class="p-media-object__details">
          <h2 class="p-media-object__title">
            <a href="/people/benhoyt">Ben Hoyt</a>
          </h2>
          <p class="p-media-object__content">Engineering Manager (Manager)</p>
          <p class="p-media-object__content">
            <strong>Group:</strong>
            <a href="/groups/Charm Engineering">Charm Engineering</a>
          </p>
          <ul class="p-media-object__meta-list">
            <li class="p-media-object__meta-list-item--email">
              Email: <a href="mailto:ben.hoyt@canonical.com">ben.hoyt@canonical.com</a>
            </li>
            <li class="p-media-object__meta-list-item--mattermost">
              Mattermost: <a href="https://chat.canonical.com/canonical/messages/@benhoyt">benhoyt</a>
            </li>
            <li class="p-media-object__meta-list-item--launchpad">
              Launchpad: <a href="https://launchpad.net/~benhoyt">benhoyt</a>
            </li>
            <li class="p-media-object__meta-list-item--github">
              Github: <a href="https://github.com/benhoyt">benhoyt</a>
            </li>
            <li class="p-media-object__meta-list-item--manager">
              Manager: <a href="/people/Jon Seager">Jon Seager</a>
            </li>
          </ul>
        </div>
      </div>
    </li>
    <ol type="1">
      <li>
        <div class="p-media-object">
          <div class="p-media-object__details">
            <h2 class="p-media-object__title">
              <a href="/people/alice">Alice Smith</a>
            </h2>
            <p class="p-media-object__content">Software Engineer I (Profession I)</p>
            <p class="p-media-object__content">
              <strong>Group:</strong>
              <a href="/groups/Charm Engineering">Charm Engineering</a>
            </p>
            <ul class="p-media-object__meta-list">
              <li class="p-media-object__meta-list-item--email">
                Email: <a href="mailto:alice@canonical.com">alice@canonical.com</a>
              </li>
              <li class="p-media-object__meta-list-item--launchpad">
                Launchpad: <a href="https://launchpad.net/~alice">alice</a>
              </li>
              <li class="p-media-object__meta-list-item--github">
                Github: <a href="https://github.com/alice">alice</a>
              </li>
              <li class="p-media-object__meta-list-item--manager">
                Manager: <a href="/people/Ben Hoyt">Ben Hoyt</a>
              </li>
            </ul>
          </div>
        </div>
      </li>
    </ol>
  </ol>
</ol>
</body>
</html>"#
            .to_string()
    }

    #[test]
    fn parses_people_from_html() {
        let people = parse_directory_html(&sample_html());
        assert_eq!(people.len(), 3);

        assert_eq!(people[0].display_name, "Jon Seager");
        assert_eq!(people[0].email, "jon.seager@canonical.com");
        assert_eq!(people[0].github_username, "jnsgruk");
        assert_eq!(people[0].launchpad_username, "jnsgruk");
        assert_eq!(people[0].mattermost_username.as_deref(), Some("jnsgruk"));
        assert_eq!(people[0].group.as_deref(), Some("Charm Engineering"));
        assert_eq!(
            people[0].title.as_deref(),
            Some("VP Engineering, Charm Engineering (VP)")
        );
        assert_eq!(people[0].manager_name.as_deref(), Some("Mark Shuttleworth"));
        assert_eq!(people[0].depth, 1);

        assert_eq!(people[1].display_name, "Ben Hoyt");
        assert_eq!(people[1].email, "ben.hoyt@canonical.com");
        assert_eq!(people[1].github_username, "benhoyt");
        assert_eq!(people[1].launchpad_username, "benhoyt");
        assert_eq!(people[1].group.as_deref(), Some("Charm Engineering"));
        assert_eq!(people[1].manager_name.as_deref(), Some("Jon Seager"));
        assert_eq!(people[1].depth, 2);

        assert_eq!(people[2].display_name, "Alice Smith");
        assert_eq!(people[2].depth, 3);
        assert_eq!(people[2].manager_name.as_deref(), Some("Ben Hoyt"));
    }

    #[test]
    fn skips_entries_missing_required_fields() {
        let html = r#"
        <div class="p-media-object">
          <h2 class="p-media-object__title"><a href="/people/x">No Email Person</a></h2>
          <ul>
            <li class="p-media-object__meta-list-item--github">
              Github: <a href="https://github.com/noone">noone</a>
            </li>
            <li class="p-media-object__meta-list-item--launchpad">
              Launchpad: <a href="https://launchpad.net/~noone">noone</a>
            </li>
          </ul>
        </div>"#;
        let people = parse_directory_html(html);
        assert!(people.is_empty(), "should skip entry without email");
    }

    #[test]
    fn handles_missing_optional_fields() {
        let html = r#"
        <div class="p-media-object">
          <h2 class="p-media-object__title"><a href="/people/x">Minimal Person</a></h2>
          <ul>
            <li class="p-media-object__meta-list-item--email">
              Email: <a href="mailto:min@canonical.com">min@canonical.com</a>
            </li>
            <li class="p-media-object__meta-list-item--github">
              Github: <a href="https://github.com/minperson">minperson</a>
            </li>
            <li class="p-media-object__meta-list-item--launchpad">
              Launchpad: <a href="https://launchpad.net/~minperson">minperson</a>
            </li>
          </ul>
        </div>"#;
        let people = parse_directory_html(html);
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].display_name, "Minimal Person");
        assert!(people[0].mattermost_username.is_none());
        assert!(people[0].group.is_none());
        assert!(people[0].title.is_none());
        assert!(people[0].manager_name.is_none());
        assert_eq!(people[0].depth, 0);
    }
}
