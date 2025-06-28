use rusqlite::Connection;

// 初始化数据库
pub fn initialize_db(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         
         CREATE TABLE IF NOT EXISTS temperature_records (
             id INTEGER PRIMARY KEY,
             coke_oven INTEGER NOT NULL,
             time TEXT NOT NULL,
             machine_side REAL NOT NULL,
             coke_side REAL NOT NULL,
             UNIQUE(coke_oven, time)
         );
         
         CREATE TABLE IF NOT EXISTS operation_records (
             id INTEGER PRIMARY KEY,
             coke_oven INTEGER NOT NULL,
             chamber TEXT NOT NULL,
             operation_type TEXT NOT NULL CHECK(operation_type IN ('LOAD', 'PUSH')),
             time TEXT NOT NULL,
             UNIQUE(coke_oven, chamber, time)
         );
         
         CREATE TABLE IF NOT EXISTS coking_cycles (
             id INTEGER PRIMARY KEY,
             coke_oven INTEGER NOT NULL,
             chamber TEXT NOT NULL,
             loading_time TEXT NOT NULL,
             push_time TEXT NOT NULL,
             duration_hhmm TEXT NOT NULL, 
             avg_temp_machine REAL,
             avg_temp_coke REAL,
             UNIQUE(coke_oven, chamber, push_time)
         );
         
         CREATE INDEX IF NOT EXISTS idx_temp_oven_time ON temperature_records(coke_oven, time);
         CREATE INDEX IF NOT EXISTS idx_ops_oven_chamber_time ON operation_records(coke_oven, chamber, time);
         CREATE INDEX IF NOT EXISTS idx_cycles_oven_chamber ON coking_cycles(coke_oven, chamber);"
    )
}