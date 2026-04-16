use sqlx::PgPool;
use tracing::info;

/// Run database migrations by executing the schema SQL.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    info!("running database migrations");
    let schema = include_str!("schema.sql");
    sqlx::raw_sql(schema).execute(pool).await?;
    info!("database migrations complete");
    Ok(())
}
