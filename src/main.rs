#[macro_use]
extern crate rocket;

use std::net::IpAddr;
use std::num::NonZeroU32;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use url::Url;

use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};

use rand::distributions::Alphanumeric;
use rand::Rng;

use rocket::fairing::AdHoc;
use rocket::fs::FileServer;
use rocket::futures::TryStreamExt;
use rocket::http::Status;
use rocket::response::status::{Created, Custom, NotFound};
use rocket::response::Redirect;
use rocket::serde::{json::Json, Deserialize, Serialize};
use rocket::{fairing, Build, Request, Rocket, State};

use rocket::fs::NamedFile;
use rocket_dyn_templates::{context, Template};

use rocket_db_pools::{sqlx, Connection, Database};
type Result<T, E = rocket::response::Debug<sqlx::Error>> = std::result::Result<T, E>;

const ID_LENGTH: u8 = 6;
const MAX_URL_LENGTH: usize = 2048;

#[derive(Database)]
#[database("route_db")]
struct Routes(sqlx::SqlitePool);

#[derive(Serialize, Deserialize)]
struct Route {
    #[serde(skip_deserializing)]
    id: String,
    destination: String,
    expires: i64,
}

struct RateLimitState {
    limiter: DefaultKeyedRateLimiter<IpAddr>,
}

fn generate_alphanumeric_string(length: u8) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(length as usize)
        .map(char::from)
        .collect()
}

fn get_unix_epoch() -> i64 {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards!");

    since_the_epoch.as_secs() as i64
}

fn is_valid_route_id(route_id: &str) -> bool {
    route_id.len() == ID_LENGTH as usize && route_id.chars().all(|c| c.is_ascii_alphanumeric())
}

#[post("/api/create", data = "<post>")]
async fn create(
    limiter_state: &State<RateLimitState>, mut db: Connection<Routes>, ip_addr: IpAddr,
    mut post: Json<Route>,
) -> Result<Created<Json<Route>>, Custom<String>> {
    let rate_limit_result = limiter_state.limiter.check_key(&ip_addr);
    if rate_limit_result.is_err() {
        return Err(Custom(
            Status::TooManyRequests,
            String::from("You have reached the limit of routes you may create!"),
        ));
    }

    // Check URL length
    if post.destination.len() > MAX_URL_LENGTH {
        return Err(Custom(
            Status::BadRequest,
            format!(
                "URL exceeds maximum length of {} characters",
                MAX_URL_LENGTH
            ),
        ));
    }

    post.id = String::new();

    for _ in 0..64 {
        post.id = generate_alphanumeric_string(ID_LENGTH);
        let exists_result =
            sqlx::query!("SELECT * FROM routes WHERE route_id = ? LIMIT 1", post.id)
                .fetch(&mut **db)
                .try_collect::<Vec<_>>()
                .await;

        match exists_result {
            Ok(records) if records.is_empty() => break,
            Ok(_) | Err(_) => continue,
        }
    }

    if post.id.is_empty() {
        return Err(Custom(
            Status::InternalServerError,
            String::from("Couldn't find space for your route. Sorry!"),
        ));
    }

    if get_unix_epoch() > post.expires {
        return Err(Custom(
            Status::BadRequest,
            String::from("Route set to expire in the past!"),
        ));
    }

    let parsed_destination = Url::parse(&post.destination).map_err(|_| {
        Custom(
            Status::BadRequest,
            String::from("URL destination is invalid!"),
        )
    })?;

    if parsed_destination.scheme() != "http" && parsed_destination.scheme() != "https" {
        return Err(Custom(
            Status::BadRequest,
            String::from("Non-http(s) URLs are not allowed!"),
        ));
    }

    let insert_result = sqlx::query!(
        "INSERT INTO routes (route_id, destination, expires) VALUES (?, ?, ?)",
        post.id,
        post.destination,
        post.expires
    )
    .execute(&mut **db)
    .await;

    match insert_result {
        Ok(_) => Ok(Created::new(format!("/{}", post.id)).body(post)),
        Err(_) => Err(Custom(
            Status::InternalServerError,
            String::from("Ran into issues while adding route to database."),
        )),
    }
}

#[get("/<route_id>")]
async fn route(mut db: Connection<Routes>, route_id: &str) -> Result<Redirect, NotFound<Template>> {
    if !is_valid_route_id(route_id) {
        return Err(NotFound(Template::render(
            "error",
            context! {
                error_code: 404,
                reason: "Invalid route ID."
            },
        )));
    }

    let lookup_result = sqlx::query!("SELECT * FROM routes WHERE route_id = ?", route_id)
        .fetch_one(&mut **db)
        .await;

    match lookup_result {
        Ok(record) => {
            let epoch = get_unix_epoch();

            if epoch > record.expires {
                sqlx::query!("DELETE FROM routes WHERE route_id = ?", record.route_id)
                    .execute(&mut **db)
                    .await
                    .expect("Tried to expire route that doesn't exist?");

                return Err(NotFound(Template::render(
                    "error",
                    context! {
                        error_code: 404,
                        reason: "Route doesn't exist."
                    },
                )));
            }

            Ok(Redirect::temporary(record.destination))
        }
        Err(_) => Err(NotFound(Template::render(
            "error",
            context! {
                error_code: 404,
                reason: "Route doesn't exist."
            },
        ))),
    }
}

#[get("/")]
fn index() -> Template {
    Template::render("index", context! {})
}

#[get("/favicon.ico")]
async fn favicon() -> Option<NamedFile> {
    NamedFile::open(Path::new("static/").join("favicon.ico"))
        .await
        .ok()
}

async fn run_migrations(rocket: Rocket<Build>) -> fairing::Result {
    match Routes::fetch(&rocket) {
        Some(db) => match sqlx::migrate!("db/migrations").run(&**db).await {
            Ok(_) => Ok(rocket),
            Err(e) => {
                error!("Failed to initialize SQLx database: {}", e);
                Err(rocket)
            }
        },
        None => Err(rocket),
    }
}

#[catch(404)]
fn not_found() -> Template {
    Template::render(
        "error",
        context! {
            error_code: 404,
            reason: "404 Not Found"
        },
    )
}

#[catch(429)]
fn too_many_requests() -> Template {
    Template::render(
        "error",
        context! {
            error_code: 429,
            reason: "429 Too Many Requests"
        },
    )
}

#[catch(500)]
fn internal_error(err: Status, _req: &Request) -> Template {
    Template::render(
        "error",
        context! {
            error_code: 500,
            reason: err.reason().unwrap()
        },
    )
}

#[launch]
fn rocket() -> _ {
    rocket::build()
        .attach(Routes::init())
        .attach(AdHoc::try_on_ignite("SQLx Migrations", run_migrations))
        .attach(Template::fairing())
        .manage(RateLimitState {
            limiter: RateLimiter::keyed(Quota::per_minute(NonZeroU32::new(1).unwrap())),
        })
        .mount("/static", FileServer::from("static"))
        .mount("/", routes![index, favicon, route, create])
        .register("/", catchers![not_found, too_many_requests, internal_error])
}
