use diesel::{MysqlConnection, QueryableByName, RunQueryDsl, sql_query, sql_types::Text};
use toolbox_db::Db;

#[derive(QueryableByName)]
struct SessionTimeZone {
    #[diesel(sql_type = Text)]
    time_zone: String,
}

#[test]
fn mariadb_connections_start_in_utc() {
    let Ok(url) = std::env::var("TOOLBOX_TEST_MYSQL_URL") else {
        return;
    };
    let db = Db::<MysqlConnection>::builder(url)
        .mariadb_utc()
        .max_size(1)
        .build()
        .expect("MariaDB pool");
    let mut conn = db.blocking_conn().expect("MariaDB connection");
    let session = sql_query("SELECT @@session.time_zone AS time_zone")
        .get_result::<SessionTimeZone>(&mut conn)
        .expect("MariaDB session time zone");

    assert_eq!(session.time_zone, "+00:00");
}
