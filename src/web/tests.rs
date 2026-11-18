//! The admin pages end to end: requests through the routes, a stand-in Discord side answering the
//! guild port, and every change's mod log line checked.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tower::ServiceExt as _;

use super::routes;
use crate::services::Services;
use crate::services::guild::{Ask, Channel, ChannelKind, Guild, Person, Uploaded};
use crate::services::links::Access;

const STAFF: u64 = 9;
const NOVA_PLAYER: u64 = 157;
const NOXA_PLAYER: u64 = 410;
const MOD_LOG: u64 = 60;
const BODY_MAX: usize = 1 << 20;
const FORM: &str = "application/x-www-form-urlencoded";
const UPLOADED: u64 = 77;
const WAIT: Duration = Duration::from_secs(2);

struct Admin {
    services: Arc<Services>,
    routes: Router,
    token: String,
    lines: UnboundedReceiver<String>,
}

fn channel(id: u64, name: &str, kind: ChannelKind, parent: Option<u64>) -> Channel {
    Channel { id, name: name.to_owned(), kind, parent, position: 0 }
}

// A Discord side that knows one player's name, three channels, and takes every upload.
fn admin() -> Admin {
    let services = Arc::new(Services::memory(0));
    let (guild, mut asks) = Guild::new(1);
    let (lines_in, lines) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(ask) = asks.recv().await {
            match ask {
                Ask::Audit(line) => {
                    lines_in.send(line).ok();
                }
                Ask::People { users, reply } => {
                    let known = |u: &u64| match *u {
                        NOVA_PLAYER => Some("Nicolas"),
                        NOXA_PLAYER => Some("Noxa's player"),
                        _ => None,
                    };
                    let people = users.into_iter().filter_map(|u| Some((u, Person { name: known(&u)?.to_owned(), icon: format!("https://cdn.discordapp.com/embed/avatars/{u}.png") })));
                    reply.send(people.collect()).ok();
                }
                Ask::Channels { reply } => {
                    let server = vec![
                        channel(10, "Earth", ChannelKind::Category, None),
                        channel(11, "west-city", ChannelKind::Text, Some(10)),
                        channel(12, "kame-house", ChannelKind::Text, Some(10)),
                        channel(20, "Out of character", ChannelKind::Category, None),
                        channel(21, "chat", ChannelKind::Text, Some(20)),
                        channel(MOD_LOG, "mod-log", ChannelKind::Text, None),
                    ];
                    reply.send(server).ok();
                }
                Ask::Upload { upload, reply } => {
                    let url = format!("https://cdn.discordapp.com/attachments/5/{UPLOADED}/{}.{}?ex=ffffffff", upload.stem, upload.kind.extension());
                    reply.send(Ok(Uploaded { message_id: UPLOADED, url })).ok();
                }
                Ask::Fresh { pictures, reply } => {
                    reply.send(pictures.into_iter().map(|p| (p.message_id, p.url)).collect()).ok();
                }
                Ask::Did { user, did } => {
                    lines_in.send(format!("did {user} {did:?}")).ok();
                }
            }
        }
    });
    let routes = routes(Arc::clone(&services), guild);
    let token = services.links().issue(STAFF, Access::Staff).unwrap();
    Admin { services, routes, token, lines }
}

impl Admin {
    fn at(&self, path: &str) -> String {
        if path.starts_with("/p/") {
            return path.to_owned();
        }
        format!("/a/{}{path}", self.token)
    }

    async fn call(&self, method: &str, uri: &str, body: Option<(&str, Vec<u8>)>) -> (StatusCode, HeaderMap, String) {
        let mut request = Request::builder().method(method).uri(uri);
        let bytes = match body {
            Some((kind, bytes)) => {
                request = request.header(header::CONTENT_TYPE, kind);
                bytes
            }
            None => Vec::new(),
        };
        let response = self.routes.clone().oneshot(request.body(Body::from(bytes)).unwrap()).await.unwrap();
        let (status, headers) = (response.status(), response.headers().clone());
        let body = to_bytes(response.into_body(), BODY_MAX).await.unwrap();
        (status, headers, String::from_utf8(body.to_vec()).unwrap())
    }

    async fn get(&self, path: &str) -> String {
        let (status, _, body) = self.call("GET", &self.at(path), None).await;
        assert_eq!(status, StatusCode::OK, "GET {path}");
        body
    }

    /// Posts a form and follows the redirect: the page it lands on.
    async fn post(&self, path: &str, form: &str) -> String {
        let (status, headers, body) = self.call("POST", &self.at(path), Some((FORM, form.as_bytes().to_vec()))).await;
        if status == StatusCode::OK {
            return body;
        }
        assert_eq!(status, StatusCode::SEE_OTHER, "POST {path}: {body}");
        let to = headers[header::LOCATION].to_str().unwrap().to_owned();
        let (status, _, body) = self.call("GET", &to, None).await;
        assert_eq!(status, StatusCode::OK, "GET {to}");
        body
    }

    async fn line(&mut self) -> String {
        tokio::time::timeout(WAIT, self.lines.recv()).await.expect("a mod log line").unwrap()
    }
}

#[tokio::test]
async fn a_dead_link_shows_nothing_and_a_live_one_is_locked_down() {
    let admin = admin();
    let (status, _, body) = admin.call("GET", "/a/0123456789abcdef0123456789abcdef", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("/z_moderator_admin"));

    let (status, headers, body) = admin.call("GET", &admin.at(""), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Set the mod log first"));
    assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap().contains("default-src 'none'"));
}

#[tokio::test]
async fn spirit_points_wait_for_the_mod_log_then_change_and_are_logged() {
    let mut admin = admin();
    let page = admin.post(&format!("/players/{NOVA_PLAYER}/sp"), "action=give&amount=5&memo=event").await;
    assert!(page.contains("Set the mod log first"));
    assert_eq!(admin.services.spirit().balance(NOVA_PLAYER).unwrap(), 0);

    admin.services.settings().set_mod_log(MOD_LOG).unwrap();
    let page = admin.post(&format!("/players/{NOVA_PLAYER}/sp"), "action=give&amount=5&memo=Won+the+*tournament*").await;
    assert!(page.contains("Spirit Points: 0 → 5."));
    assert!(page.contains("Nicolas"));
    assert_eq!(admin.line().await, format!("🔮 <@{STAFF}> gave **5** SP to <@{NOVA_PLAYER}>: 0 → **5** · Won the \\*tournament\\* · admin pages"));

    let page = admin.post(&format!("/players/{NOVA_PLAYER}/sp"), "action=take&amount=9&memo=oops").await;
    assert!(page.contains("They have 5 SP."));
    let page = admin.post(&format!("/players/{NOVA_PLAYER}/sp"), "action=grant&amount=9&memo=x").await;
    assert!(page.contains("Pick give, take or set."));

    // The notice shows once: a reload is the plain page.
    assert!(!admin.get(&format!("/players/{NOVA_PLAYER}")).await.contains("Pick give"));
    assert!(admin.get("/players").await.contains(&format!("<a href=\"/a/{}/players/{NOVA_PLAYER}\">Nicolas", admin.token)));
}

#[tokio::test]
async fn a_sheet_saves_whole_and_a_refused_save_keeps_what_was_typed() {
    let mut admin = admin();
    admin.services.settings().set_mod_log(MOD_LOG).unwrap();
    let page = admin.post(&format!("/players/{NOVA_PLAYER}/characters"), "slot=1&name=Captain+Nova").await;
    assert!(page.contains("Captain Nova is in slot 1."));
    assert_eq!(admin.line().await, format!("👤 <@{STAFF}> added **Captain Nova** to <@{NOVA_PLAYER}>'s slot 1 · admin pages"));
    let nova = admin.services.characters().of_player(NOVA_PLAYER).unwrap().remove(0);

    let sheet = format!("/characters/{}", nova.id);
    let page = admin.post(&sheet, "name=Captain+Nova&race=Earthling&tier=9&title=EDF+Commander&posting=on").await;
    assert!(page.contains("Saved: race, tier, title."));
    assert_eq!(admin.line().await, format!("👤 <@{STAFF}> edited **Captain Nova**'s sheet (<@{NOVA_PLAYER}>): race, tier, title · admin pages"));

    let page = admin.post(&sheet, "name=Captain+Nova&race=Saiyan&tier=over+9000&posting=on").await;
    assert!(page.contains("Tier: a whole number"));
    assert!(page.contains("value=\"Saiyan\""));
    assert_eq!(admin.services.sheets().get(nova.id).unwrap().unwrap().race.as_deref(), Some("Earthling"));
}

#[tokio::test]
async fn npcs_join_the_cast_and_open_on_their_sheet() {
    let mut admin = admin();
    admin.services.settings().set_mod_log(MOD_LOG).unwrap();
    let page = admin.post("/npcs", "name=King+Yenma&prefix=yen").await;
    assert!(page.contains("King Yenma joined the cast."));
    assert!(page.contains("Save sheet"));
    assert_eq!(admin.line().await, format!("🎭 <@{STAFF}> added NPC **King Yenma** (`yen:`) · admin pages"));
    let page = admin.post("/npcs", "name=Copycat&prefix=YEN").await;
    assert!(page.contains("That prefix is King Yenma&#39;s."));
    assert!(admin.get("/npcs").await.contains("King Yenma"));
}

#[tokio::test]
async fn mcguffins_are_made_given_taken_back_and_retired() {
    let mut admin = admin();
    admin.services.settings().set_mod_log(MOD_LOG).unwrap();
    admin.post(&format!("/players/{NOVA_PLAYER}/characters"), "slot=1&name=Captain+Nova").await;
    admin.line().await;
    let nova = admin.services.characters().of_player(NOVA_PLAYER).unwrap().remove(0);

    let page = admin.post("/mcguffins", "name=Star+Key&emoji=%F0%9F%97%9D&holding=one&description=Opens+a+door").await;
    assert!(page.contains("Star Key made."));
    assert!(admin.line().await.contains("made McGuffin **Star Key**"));
    let key = admin.services.mcguffins().all().unwrap().remove(0);

    let page = admin.post(&format!("/characters/{}/mcguffins", nova.id), &format!("mcguffin={}", key.id)).await;
    assert!(page.contains("Captain Nova now holds Star Key."));
    assert_eq!(admin.line().await, format!("🔮 <@{STAFF}> gave **Star Key** to **Captain Nova** (<@{NOVA_PLAYER}>) · admin pages"));

    let page = admin.post(&format!("/mcguffins/{}/take", key.id), &format!("character={}", nova.id)).await;
    assert!(page.contains("Took Star Key back from Captain Nova."));
    admin.line().await;

    let page = admin.post(&format!("/mcguffins/{}/retire", key.id), "state=retire").await;
    assert!(page.contains("Star Key retired."));
    assert!(admin.line().await.contains("retired McGuffin **Star Key**"));
    let page = admin.post(&format!("/mcguffins/{}/give", key.id), &format!("character={}", nova.id)).await;
    assert!(page.contains("retired. Restore it first."));
}

#[tokio::test]
async fn dragon_ball_areas_rolls_and_hiding_places_are_set_from_the_page() {
    let mut admin = admin();
    admin.services.settings().set_mod_log(MOD_LOG).unwrap();
    let page = admin.get("/balls/earth").await;
    assert!(page.contains("name=\"area_10\""));
    assert!(page.contains("#west-city"));

    let page = admin.post("/balls/earth/areas", "area_10=on&area_999=on").await;
    assert!(page.contains("Search areas saved: 1 changes."));
    assert_eq!(admin.line().await, format!("🐉 <@{STAFF}> changed Earth's search areas: + <#10> (category) · admin pages"));
    assert!(page.contains("Searches count in 2 text channels."));

    let page = admin.post("/balls/earth/need", "need=75").await;
    assert!(page.contains("A search now needs 75 or more."));
    admin.line().await;

    let page = admin.post("/balls/earth/place", "star=4&channel=12").await;
    assert!(page.contains("4★ hidden in #kame-house."));
    assert_eq!(admin.line().await, format!("🐉 <@{STAFF}> hid Earth 4★ in <#12> · admin pages"));
    let page = admin.post("/balls/earth/place", "star=4&channel=10").await;
    assert!(page.contains("Pick a text channel."));

    let page = admin.post("/balls/earth/scatter", "").await;
    assert!(page.contains("7 balls scattered."));
    let line = admin.line().await;
    assert!(line.starts_with(&format!("🐉 <@{STAFF}> scattered Earth: 1★ <#1")));
    let view = admin.services.balls().view(crate::domain::dragon_balls::BallSet::Earth).unwrap();
    assert!(view.balls.iter().all(|b| matches!(b.channel_id, Some(11 | 12))));
}

fn multipart(bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "noxa";
    let mut body = format!("--{boundary}\r\ncontent-disposition: form-data; name=\"picture\"; filename=\"nova.png\"\r\ncontent-type: image/png\r\n\r\n").into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[tokio::test]
async fn a_picture_uploads_in_the_background_and_becomes_the_sheets() {
    let mut admin = admin();
    admin.services.settings().set_mod_log(MOD_LOG).unwrap();
    admin.post(&format!("/players/{NOVA_PLAYER}/characters"), "slot=1&name=Captain+Nova").await;
    admin.line().await;
    let nova = admin.services.characters().of_player(NOVA_PLAYER).unwrap().remove(0);
    let path = admin.at(&format!("/characters/{}/picture", nova.id));

    let (kind, body) = multipart(b"<svg onload=alert(1)>");
    let (status, _, _) = admin.call("POST", &path, Some((&kind, body))).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(admin.get(&format!("/characters/{}", nova.id)).await.contains("not a PNG, JPEG, GIF or WebP"));

    let (kind, body) = multipart(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR");
    let (status, _, _) = admin.call("POST", &path, Some((&kind, body))).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(admin.line().await, format!("🖼️ <@{STAFF}> uploaded a picture for **Captain Nova** (<@{NOVA_PLAYER}>) · admin pages"));

    let row = admin.services.sheets().get(nova.id).unwrap().unwrap();
    assert_eq!(row.picture_message, Some(UPLOADED));
    let page = admin.get(&format!("/characters/{}", nova.id)).await;
    assert!(page.contains(&format!("src=\"https://cdn.discordapp.com/attachments/5/{UPLOADED}/captain-nova.png?ex=ffffffff\"")));
    assert!(page.contains("Uploaded picture in use."));
}

#[tokio::test]
async fn feats_are_logged_archived_and_restored_from_the_sheet() {
    let mut admin = admin();
    admin.services.settings().set_mod_log(MOD_LOG).unwrap();
    admin.post(&format!("/players/{NOVA_PLAYER}/characters"), "slot=1&name=Captain+Nova").await;
    admin.line().await;
    let nova = admin.services.characters().of_player(NOVA_PLAYER).unwrap().remove(0);

    let sheet = format!("/characters/{}", nova.id);
    let page = admin.post(&format!("{sheet}/feats"), "kind=victory&counts=fight&note=Beat+Retsu&link=https://discord.com/channels/2/3/4").await;
    assert!(page.contains("That post is on another server."));
    let page = admin.post(&format!("{sheet}/feats"), "kind=victory&counts=fight&note=Beat+Retsu&link=https://discord.com/channels/1/3/4").await;
    assert!(page.contains("F-0001 logged."));
    assert!(page.contains("href=\"https://discord.com/channels/1/3/4\""));
    assert!(admin.line().await.contains("logged ⚔️ Victory **F-0001**"));

    let page = admin.post("/feats/1/archive", "reason=duplicate").await;
    assert!(page.contains("F-0001 archived."));
    assert_eq!(admin.line().await, format!("🔒 <@{STAFF}> archived **F-0001** (Captain Nova) · duplicate · admin pages"));
    let page = admin.post("/feats/1/restore", "").await;
    assert!(page.contains("F-0001 restored."));
    admin.line().await;
}

#[tokio::test]
async fn a_player_link_opens_only_its_own_pages_and_a_staff_link_only_the_admin_ones() {
    let admin = admin();
    let player = admin.services.links().issue(NOVA_PLAYER, Access::Player).unwrap();
    let (status, _, _) = admin.call("GET", &format!("/a/{player}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = admin.call("GET", &format!("/a/{player}/players"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = admin.call("POST", &format!("/a/{player}/players/{NOVA_PLAYER}/sp"), Some((FORM, b"action=give&amount=5&memo=x".to_vec()))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = admin.call("GET", &format!("/p/{}", admin.token), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, body) = admin.call("GET", &format!("/p/{player}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("no characters yet"));
}

#[tokio::test]
async fn a_player_sees_their_cards_picks_who_they_play_and_sees_others_without_staff_notes() {
    let mut admin = admin();
    admin.services.settings().set_mod_log(MOD_LOG).unwrap();
    admin.post(&format!("/players/{NOVA_PLAYER}/characters"), "slot=1&name=Captain+Nova").await;
    admin.post(&format!("/players/{NOVA_PLAYER}/characters"), "slot=2&name=Nicolas").await;
    admin.post(&format!("/players/{NOXA_PLAYER}/characters"), "slot=1&name=Noxa").await;
    admin.post(&format!("/players/{NOVA_PLAYER}/sp"), "action=give&amount=12&memo=Tournament").await;
    for _ in 0..4 {
        admin.line().await;
    }
    let nova = admin.services.characters().of_player(NOVA_PLAYER).unwrap();
    let noxa = admin.services.characters().of_player(NOXA_PLAYER).unwrap().remove(0);
    admin.post(&format!("/characters/{}", noxa.id), "name=Noxa&race=Bio-Android&notes=Secret+plot+twist").await;
    admin.line().await;

    let player = admin.services.links().issue(NOVA_PLAYER, Access::Player).unwrap();
    let home = format!("/p/{player}");
    let page = admin.get(&home).await;
    assert!(page.contains("<h1>Nicolas</h1>"));
    assert!(page.contains("🔮 12 Spirit Points"));
    assert!(page.contains("Tournament"));
    assert!(page.contains("Play as Captain Nova"));
    assert!(page.contains("src=\"https://cdn.discordapp.com/embed/avatars/157.png\""));

    let page = admin.post(&format!("{home}/play"), &format!("character={}", nova[1].id)).await;
    assert!(page.contains("You play Nicolas now."));
    assert!(page.contains("✅ Playing"));
    assert_eq!(admin.services.characters().active(NOVA_PLAYER).unwrap().map(|c| c.id), Some(nova[1].id));

    // Someone else's character is not theirs to play.
    let page = admin.post(&format!("{home}/play"), &format!("character={}", noxa.id)).await;
    assert!(page.contains("Pick one of your own characters."));
    assert_eq!(admin.services.characters().active(NOXA_PLAYER).unwrap(), None);

    let page = admin.get(&format!("{home}/players")).await;
    assert!(page.contains("Noxa&#39;s player"));
    let page = admin.get(&format!("{home}/players/{NOXA_PLAYER}")).await;
    assert!(page.contains("Bio-Android"));
    assert!(!page.contains("Secret plot twist"));
    assert!(!page.contains("Play as"));
    let (status, headers, _) = admin.call("GET", &format!("{home}/players/{NOVA_PLAYER}"), None).await;
    assert_eq!((status, headers[header::LOCATION].to_str().unwrap()), (StatusCode::SEE_OTHER, home.as_str()));
}

#[tokio::test]
async fn mcguffins_lie_in_a_world_a_channel_or_a_rolled_roleplay_channel_and_leave_it_when_given() {
    let mut admin = admin();
    admin.services.settings().set_mod_log(MOD_LOG).unwrap();
    admin.post(&format!("/players/{NOVA_PLAYER}/characters"), "slot=1&name=Captain+Nova").await;
    admin.line().await;
    let nova = admin.services.characters().of_player(NOVA_PLAYER).unwrap().remove(0);

    // Worlds: the server's categories, read now; a category it does not have is never kept.
    let page = admin.get("/worlds").await;
    assert!(page.contains("name=\"world_10\""));
    assert!(page.contains("Out of character"));
    let page = admin.post("/worlds", "world_10=on&world_999=on").await;
    assert!(page.contains("1 worlds, 2 roleplay channels."));
    assert_eq!(admin.line().await, format!("🌍 <@{STAFF}> changed the worlds: + <#10> · admin pages"));

    admin.post("/mcguffins", "name=Star+Key&holding=one").await;
    admin.line().await;
    let key = admin.services.mcguffins().all().unwrap().remove(0);
    let at = format!("/mcguffins/{}/place", key.id);

    let page = admin.post(&at, "mode=world&world=20").await;
    assert!(page.contains("Pick a world: a category ticked on the Worlds page"));
    let page = admin.post(&at, "mode=world&world=10").await;
    assert!(page.contains("Star Key now lies in world Earth."));
    assert!(page.contains("🌍 world <b>Earth</b>"));
    assert_eq!(admin.line().await, format!("🔮 <@{STAFF}> put McGuffin **Star Key** in world <#10> (was nowhere) · admin pages"));

    let page = admin.post(&at, "mode=channel&channel=60").await;
    assert!(page.contains("Star Key now lies in #mod-log."));
    admin.line().await;
    let page = admin.post(&at, "mode=channel&channel=10").await;
    assert!(page.contains("Pick a text channel the server has."));

    // A roll lands in a world's text channel, never out of character.
    let page = admin.post(&at, "mode=random").await;
    assert!(page.contains("Re-roll"));
    let line = admin.line().await;
    assert!(line.contains("at random in <#11>") || line.contains("at random in <#12>"), "{line}");
    assert!(line.contains("(of 2 roleplay channels)"));
    assert!(admin.get("/mcguffins").await.contains("🎲 #"));

    // Given, a one-holder McGuffin is carried: it no longer lies anywhere.
    admin.post(&format!("/characters/{}/mcguffins", nova.id), &format!("mcguffin={}", key.id)).await;
    let line = admin.line().await;
    assert!(line.contains("; it no longer lies at random in <#1"), "{line}");
    assert_eq!(admin.services.mcguffins().get(key.id).unwrap().unwrap().place, None);
}

#[tokio::test]
async fn a_player_sets_their_prefix_and_expressions_on_their_page_and_the_tutorial_hears_it() {
    let mut admin = admin();
    admin.services.settings().set_mod_log(MOD_LOG).unwrap();
    admin.post(&format!("/players/{NOVA_PLAYER}/characters"), "slot=1&name=Captain+Nova").await;
    admin.post(&format!("/players/{NOXA_PLAYER}/characters"), "slot=1&name=Noxa").await;
    admin.line().await;
    admin.line().await;
    let nova = admin.services.characters().of_player(NOVA_PLAYER).unwrap().remove(0);
    let noxa = admin.services.characters().of_player(NOXA_PLAYER).unwrap().remove(0);
    let home = format!("/p/{}", admin.services.links().issue(NOVA_PLAYER, Access::Player).unwrap());

    let page = admin.post(&format!("{home}/characters/{}/posting", nova.id), "prefix=Nova:&posting=on").await;
    assert!(page.contains("Captain Nova posts with nova: text"));
    assert_eq!(admin.line().await, format!("did {NOVA_PLAYER} PrefixSet"));
    let page = admin.post(&format!("{home}/characters/{}/posting", noxa.id), "prefix=nx&posting=on").await;
    assert!(page.contains("Pick one of your own characters."));

    let page = admin.post(&format!("{home}/characters/{}/expressions", nova.id), "name=happy&url=https://i.imgur.com/h.png&color=%23C0392B").await;
    assert!(page.contains("Saved the happy expression."));
    assert!(page.contains("<code>nova:happy</code>"));
    let page = admin.post(&format!("{home}/characters/{}/expressions", noxa.id), "name=happy&url=https://i.imgur.com/h.png").await;
    assert!(page.contains("Pick one of your own characters."));
    let page = admin.post(&format!("{home}/characters/{}/expressions/remove", nova.id), "name=happy").await;
    assert!(page.contains("Removed."));

    // Staff edit any sheet's expressions, logged.
    let page = admin.post(&format!("/characters/{}/expressions", noxa.id), "name=angry&url=https://i.imgur.com/a.png").await;
    assert!(page.contains("Saved Noxa&#39;s angry expression."));
    assert!(admin.line().await.contains("saved expression **angry** for **Noxa**"));
}
