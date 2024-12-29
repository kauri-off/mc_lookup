use std::{io, net::IpAddr, sync::Arc, time::Duration};

use chrono::{Local, Timelike};
use colored::Colorize;
use database::{DatabaseWrapper, PlayerInsert, ServerInsert, ServerModel};
use diesel::{insert_into, query_dsl::methods::SelectDsl, ExpressionMethods, RunQueryDsl, SelectableHelper};
use mc_lookup::{check_server, generate_random_ip};
use server_actions::{with_connection::get_extra_data, without_connection::get_status};
use tokio::sync::Mutex;

mod conn_wrapper;
mod database;
mod packets;
mod schema;
mod server_actions;

const MAX_WORKERS: usize = 150;

pub async fn handle_valid_ip(
    ip: &IpAddr,
    port: u16,
    db: Arc<Mutex<DatabaseWrapper>>,
) -> io::Result<()> {
    let status = get_status(format!("{}", ip), port).await?;

    let extra_data =
        get_extra_data(format!("{}", ip), port, status.version.protocol as i32).await?;

    let server_insert = ServerInsert {
        addr: &format!("{}", ip),
        online: status.players.online as i32,
        max: status.players.max as i32,
        version_name: &status.version.name,
        protocol: status.version.protocol as i32,
        license: extra_data.license,
        white_list: extra_data.white_list,
    };

    let server: ServerModel = insert_into(schema::server::dsl::server)
        .values(server_insert)
        .returning(ServerModel::as_returning())
        .get_result(&mut db.lock().await.conn)
        .unwrap();

    for player in status.players.sample.unwrap_or_default() {
        let player_model = PlayerInsert {
            uuid: &player.id,
            name: &player.name,
            server_id: server.id,
        };

        insert_into(schema::players::dsl::players)
            .values(&player_model)
            .execute(&mut db.lock().await.conn)
            .unwrap();
    }

    let timestamp = Local::now().format("%H:%M:%S").to_string();

    println!(
        "[{}] {} {} | {} {} | {} {}/{} | {}",
        timestamp,
        "🌐 Address:".blue(),
        ip,
        "🛠  Version:".yellow(),
        status.version.name,
        "👥 Players:".green(),
        status.players.online,
        status.players.max,
        if extra_data.license {
            "yes".red()
        } else {
            "no".green()
        }
    );
    Ok(())
}

async fn worker(db: Arc<Mutex<DatabaseWrapper>>) {
    loop {
        let addr = generate_random_ip();

        if check_server(&IpAddr::V4(addr), 25565).await {
            if let Err(_) = handle_valid_ip(&IpAddr::V4(addr), 25565, db.clone()).await {
                // println!("Err: {}", addr);
            }
        }
    }
}

async fn updater(db: Arc<Mutex<DatabaseWrapper>>) {
    loop {
        println!("Updating...");

        let servers: Vec<ServerModel> = schema::server::dsl::server
            .select(ServerModel::as_select())
            .load(&mut db.lock().await.conn)
            .unwrap();

        for server in servers {
            let status = get_status(server.addr, 25565).await;
            if status.is_err() {
                continue;
            }

            for player in status.unwrap().players.sample.unwrap_or_default() {
                let player_model = PlayerInsert {
                    uuid: &player.id,
                    name: &player.name,
                    server_id: server.id,
                };

                insert_into(schema::players::dsl::players)
                    .values(&player_model)
                    .on_conflict((schema::players::dsl::name, schema::players::dsl::server_id))
                    .do_update()
                    .set(
                        schema::players::dsl::last_seen
                            .eq(Local::now().naive_local().with_nanosecond(0).unwrap()),
                    )
                    .execute(&mut db.lock().await.conn)
                    .unwrap();
            }
        }

        tokio::time::sleep(Duration::from_secs(600)).await;
    }
}

#[tokio::main]
async fn main() {
    let now = Local::now();
    let time_string = now.format("%Y-%m-%d %H:%M:%S").to_string();

    println!(
        "🕒 [{}] | 🌟 mc_lookup | 🚀 Started ",
        time_string.red().bold()
    );

    let db = Arc::new(Mutex::new(DatabaseWrapper::establish()));

    let updater_thread = tokio::spawn(updater(db.clone()));
    let mut workers = vec![];

    for _ in 0..MAX_WORKERS {
        workers.push(tokio::spawn(worker(db.clone())));
    }

    for task in workers {
        let _ = task.await;
    }

    updater_thread.await.unwrap();
}
