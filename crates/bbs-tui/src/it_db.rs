#[cfg(test)]
mod it_db {
    use crate::data;
    use rand::Rng;
    use sqlx::postgres::PgPoolOptions;

    #[tokio::test]
    async fn leave_room_drops_membership() -> anyhow::Result<()> {
        // Skip if DATABASE_URL not set
        let database_url = match std::env::var("DATABASE_URL") {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };

        let pool = PgPoolOptions::new().max_connections(5).connect(&database_url).await?;
        sqlx::migrate!().run(&pool).await?;

        // Random user and room
        let fp = format!("test-fp-{:08x}", rand::thread_rng().gen::<u32>());
        let user = data::upsert_user_by_fp(&pool, &fp, "ed25519").await?;
        let room_name = format!("it-{:08x}", rand::thread_rng().gen::<u32>());
        let room = data::ensure_room_exists(&pool, &room_name, user.id).await?;

        // Join
        data::join_room(&pool, room.id, user.id).await?;
        let joined = data::list_joined_rooms(&pool, user.id).await?;
        assert!(joined.iter().any(|r| r.id == room.id));

        // Leave
        let dropped = data::leave_room(&pool, room.id, user.id).await?;
        assert!(dropped);
        let joined2 = data::list_joined_rooms(&pool, user.id).await?;
        assert!(!joined2.iter().any(|r| r.id == room.id));

        // Idempotent leave
        let dropped2 = data::leave_room(&pool, room.id, user.id).await?;
        assert!(!dropped2);
        Ok(())
    }
}

