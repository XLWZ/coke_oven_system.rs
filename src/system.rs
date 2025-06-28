use crate::db::initialize_db;
use crate::models::{TempRecord, TimeTempPoint};
use crate::oven::{initialize_ovens, CokeOven};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

// 系统状态
pub struct CokeOvenSystem {
    pub conn: Connection,
    pub ovens: HashMap<i32, CokeOven>,
}

impl CokeOvenSystem {
    pub fn new(db_path: &str) -> Result<Self, String> {
        let conn = Connection::open(db_path).map_err(|e| format!("无法打开数据库: {}", e))?;
        initialize_db(&conn).map_err(|e| format!("数据库初始化失败: {}", e))?;
        let ovens = initialize_ovens();
        Ok(Self { conn, ovens })
    }

    pub fn record_temperature(
        &mut self,
        coke_oven: i32,
        time: &str,
        machine_temp: f64,
        coke_temp: f64,
    ) -> Result<(), String> {
        if !self.ovens.contains_key(&coke_oven) {
            return Err(format!("无效焦炉编号: {}", coke_oven));
        }

        let _time_dt = crate::models::parse_time(time)?;

        self.conn
            .execute(
                "INSERT INTO temperature_records (coke_oven, time, machine_side, coke_side)
             VALUES (?1, ?2, ?3, ?4)",
                params![coke_oven, time, machine_temp, coke_temp],
            )
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn record_operation(
        &mut self,
        coke_oven: i32,
        chamber: &str,
        op_type: &str,
        time: &str,
    ) -> Result<(), String> {
        let oven = self
            .ovens
            .get(&coke_oven)
            .ok_or_else(|| format!("无效焦炉编号: {}", coke_oven))?;

        if !oven.is_valid_chamber(chamber) {
            return Err(format!("焦炉{}中无效的炭化室: {}", coke_oven, chamber));
        }

        if op_type != "LOAD" && op_type != "PUSH" {
            return Err("无效操作类型".to_string());
        }

        let _time_dt = crate::models::parse_time(time)?;

        self.conn
            .execute(
                "INSERT INTO operation_records (coke_oven, chamber, operation_type, time)
             VALUES (?1, ?2, ?3, ?4)",
                params![coke_oven, chamber, op_type, time],
            )
            .map_err(|e| e.to_string())?;

        if op_type == "PUSH" {
            self.try_calculate_coking_cycle(coke_oven, chamber, time)
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    fn try_calculate_coking_cycle(
        &mut self,
        coke_oven: i32,
        chamber: &str,
        push_time: &str,
    ) -> Result<(), rusqlite::Error> {
        let loading_time: Option<String> = self
            .conn
            .query_row(
                "SELECT time FROM operation_records 
             WHERE coke_oven = ?1 
               AND chamber = ?2 
               AND operation_type = 'LOAD' 
               AND time < ?3
             ORDER BY time DESC LIMIT 1",
                params![coke_oven, chamber, push_time],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(loading_time) = loading_time {
            let load_dt = crate::models::parse_time(&loading_time)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let push_dt =
                crate::models::parse_time(push_time).map_err(|_| rusqlite::Error::InvalidQuery)?;

            let duration = push_dt.signed_duration_since(load_dt);
            let duration_minutes = duration.num_minutes() as i32;

            // 转换为 HH:mm 格式
            let duration_hhmm = minutes_to_hhmm(duration_minutes);

            let (avg_machine, avg_coke) =
                match self.calculate_avg_temperature(coke_oven, &loading_time, push_time) {
                    Ok((m, c)) => (Some(m), Some(c)),
                    Err(e) => {
                        eprintln!("计算平均温度失败：{}", e);
                        (None, None)
                    }
                };

            self.conn.execute(
                "INSERT INTO coking_cycles (
                    coke_oven, chamber, loading_time, push_time, 
                    duration_hhmm, avg_temp_machine, avg_temp_coke
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    coke_oven,
                    chamber,
                    loading_time,
                    push_time,
                    duration_hhmm,
                    avg_machine,
                    avg_coke
                ],
            )?;
        }

        Ok(())
    }

    // 计算装煤到推焦期间的平均温度（通过积分）
    fn calculate_avg_temperature(
        &self,
        coke_oven: i32,
        start_time: &str,
        end_time: &str,
    ) -> Result<(f64, f64), rusqlite::Error> {
        let start_dt =
            crate::models::parse_time(start_time).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let end_dt =
            crate::models::parse_time(end_time).map_err(|_| rusqlite::Error::InvalidQuery)?;

        // 查询装煤时间点前后的温度记录
        let prev_start = self.get_nearest_temp_record(coke_oven, start_time, true)?;
        let next_start = self.get_nearest_temp_record(coke_oven, start_time, false)?;
        let prev_end = self.get_nearest_temp_record(coke_oven, end_time, true)?;
        let next_end = self.get_nearest_temp_record(coke_oven, end_time, false)?;

        // 获取中间记录
        let middle_records = self.get_temp_records_in_range(coke_oven, start_time, end_time)?;

        // 计算边界点温度
        let start_temp = crate::models::interpolate_temp(&prev_start, &next_start, start_dt)
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        let end_temp = crate::models::interpolate_temp(&prev_end, &next_end, end_dt)
            .ok_or(rusqlite::Error::QueryReturnedNoRows)?;

        // 构建时间点序列
        let mut points = Vec::new();
        points.push(TimeTempPoint {
            time: start_dt,
            machine: start_temp.0,
            coke: start_temp.1,
        });

        for record in middle_records {
            points.push(TimeTempPoint {
                time: record.time,
                machine: record.machine_side,
                coke: record.coke_side,
            });
        }

        points.push(TimeTempPoint {
            time: end_dt,
            machine: end_temp.0,
            coke: end_temp.1,
        });

        // 计算积分
        let (total_machine_area, total_coke_area, total_duration) = calculate_integral(&points);

        if total_duration == 0.0 {
            Ok((points[0].machine, points[0].coke))
        } else {
            let avg_machine = total_machine_area / total_duration;
            let avg_coke = total_coke_area / total_duration;
            Ok((avg_machine, avg_coke))
        }
    }

    // 辅助方法：获取最近温度记录
    fn get_nearest_temp_record(
        &self,
        coke_oven: i32,
        time: &str,
        before: bool,
    ) -> Result<Option<TempRecord>, rusqlite::Error> {
        let query = if before {
            "SELECT time, machine_side, coke_side FROM temperature_records
             WHERE coke_oven = ?1 AND time <= ?2
             ORDER BY time DESC LIMIT 1"
        } else {
            "SELECT time, machine_side, coke_side FROM temperature_records
             WHERE coke_oven = ?1 AND time > ?2
             ORDER BY time ASC LIMIT 1"
        };

        self.conn
            .query_row(query, params![coke_oven, time], |row| {
                let time_str: String = row.get(0)?;
                let time_dt = crate::models::parse_time(&time_str)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok(TempRecord {
                    time: time_dt,
                    machine_side: row.get(1)?,
                    coke_side: row.get(2)?,
                })
            })
            .optional()
    }

    // 辅助方法：获取时间范围内的温度记录
    fn get_temp_records_in_range(
        &self,
        coke_oven: i32,
        start: &str,
        end: &str,
    ) -> Result<Vec<TempRecord>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT time, machine_side, coke_side FROM temperature_records
            WHERE coke_oven = ?1 AND time > ?2 AND time < ?3
            ORDER BY time ASC",
        )?;

        let records = stmt
            .query_map(params![coke_oven, start, end], |row| {
                let time_str: String = row.get(0)?;
                let time_dt = crate::models::parse_time(&time_str)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok(TempRecord {
                    time: time_dt,
                    machine_side: row.get(1)?,
                    coke_side: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }
}

// 计算积分面积和总时长
fn calculate_integral(points: &[TimeTempPoint]) -> (f64, f64, f64) {
    let mut total_machine = 0.0;
    let mut total_coke = 0.0;
    let mut total_duration = 0.0;

    for i in 0..points.len() - 1 {
        let p1 = &points[i];
        let p2 = &points[i + 1];
        if p1.time == p2.time {
            continue;
        }
        let duration = (p2.time - p1.time).num_seconds() as f64 / 60.0;
        total_machine += (p1.machine + p2.machine) * duration / 2.0;
        total_coke += (p1.coke + p2.coke) * duration / 2.0;
        total_duration += duration;
    }

    (total_machine, total_coke, total_duration)
}

// 辅助函数：分钟转 HH:mm
fn minutes_to_hhmm(minutes: i32) -> String {
    let hours = minutes / 60;
    let minutes = minutes % 60;
    format!("{:02}:{:02}", hours, minutes)
}

// 测试代码
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    // 创建测试临时数据库
    fn setup_test_db() -> (NamedTempFile, CokeOvenSystem) {
        let temp_db = NamedTempFile::new().unwrap();
        let db_path = temp_db.path().to_str().unwrap();
        let system = CokeOvenSystem::new(db_path).unwrap();
        (temp_db, system)
    }

    #[test]
    fn test_minutes_to_hhmm() {
        assert_eq!(minutes_to_hhmm(0), "00:00");
        assert_eq!(minutes_to_hhmm(59), "00:59");
        assert_eq!(minutes_to_hhmm(60), "01:00");
        assert_eq!(minutes_to_hhmm(119), "01:59");
        assert_eq!(minutes_to_hhmm(120), "02:00");
        assert_eq!(minutes_to_hhmm(1709), "28:29"); // 样本中正确的 28 小时 29 分钟
        assert_eq!(minutes_to_hhmm(1724), "28:44"); // 样本中错误的 28 小时 44 分钟
    }

    #[test]
    fn test_record_temperature() {
        let (_temp_db, mut system) = setup_test_db();

        // 有效记录
        assert!(system
            .record_temperature(1, "2025-06-18 08:00", 1350.0, 1360.0)
            .is_ok());

        // 无效焦炉编号
        assert!(system
            .record_temperature(4, "2025-06-18 08:00", 1350.0, 1360.0)
            .is_err());

        // 无效时间格式
        assert!(system
            .record_temperature(1, "invalid_time", 1350.0, 1360.0)
            .is_err());
    }

    #[test]
    fn test_record_operation() {
        let (_temp_db, mut system) = setup_test_db();

        // 有效装煤操作
        assert!(system
            .record_operation(1, "1#", "LOAD", "2025-06-18 08:00")
            .is_ok());

        // 有效推焦操作
        assert!(system
            .record_operation(1, "1#", "PUSH", "2025-06-19 12:45")
            .is_ok());

        // 无效炭化室
        assert!(system
            .record_operation(1, "999#", "LOAD", "2025-06-18 08:00")
            .is_err());

        // 无效操作类型
        assert!(system
            .record_operation(1, "1#", "INVALID", "2025-06-18 08:00")
            .is_err());
    }

    #[test]
    fn test_coking_cycle_calculation() {
        let (_temp_db, mut system) = setup_test_db();

        // 添加温度记录
        system
            .record_temperature(1, "2025-06-18 08:00", 1350.0, 1360.0)
            .unwrap();
        system
            .record_temperature(1, "2025-06-19 12:00", 1400.0, 1410.0)
            .unwrap();
        system
            .record_temperature(1, "2025-06-19 13:00", 1420.0, 1430.0)
            .unwrap();

        // 添加装煤和推焦操作
        system
            .record_operation(1, "48#", "LOAD", "2025-06-18 08:16")
            .unwrap();
        system
            .record_operation(1, "48#", "PUSH", "2025-06-19 12:45")
            .unwrap();

        // 检查结焦周期是否正确计算
        let conn = &system.conn;
        let mut stmt = conn
            .prepare(
                "SELECT loading_time, push_time, duration_hhmm
            FROM coking_cycles
            WHERE chamber = '48#'",
            )
            .unwrap();

        let mut rows = stmt.query([]).unwrap();
        let row = rows.next().unwrap().unwrap();

        let loading_time: String = row.get(0).unwrap();
        let push_time: String = row.get(1).unwrap();
        let duration_hhmm: String = row.get(2).unwrap();

        assert_eq!(loading_time, "2025-06-18 08:16");
        assert_eq!(push_time, "2025-06-19 12:45");

        // 验证时间差计算
        let load_dt = crate::models::parse_time(&loading_time).unwrap();
        let push_dt = crate::models::parse_time(&push_time).unwrap();
        let duration = push_dt.signed_duration_since(load_dt);
        let minutes = duration.num_minutes() as i32;

        // 计算预期值：28 小时 29 分钟
        let expected_minutes = 28 * 60 + 29;
        assert_eq!(
            minutes, expected_minutes,
            "实际分钟数：{}，预期分钟数：{}",
            minutes, expected_minutes
        );

        let expected_duration = minutes_to_hhmm(expected_minutes);
        assert_eq!(
            duration_hhmm, expected_duration,
            "实际持续时间：{}，预期持续时间：{}",
            duration_hhmm, expected_duration
        )
    }

    #[test]
    fn test_get_nearest_temp_record() {
        let (_temp_db, mut system) = setup_test_db();

        // 添加温度记录
        system
            .record_temperature(1, "2025-06-18 08:00", 1350.0, 1360.0)
            .unwrap();
        system
            .record_temperature(1, "2025-06-18 10:00", 1360.0, 1370.0)
            .unwrap();
        system
            .record_temperature(1, "2025-06-18 12:00", 1370.0, 1380.0)
            .unwrap();

        // 测试在时间点之前的最近记录
        let record = system
            .get_nearest_temp_record(1, "2025-06-18 09:00", true)
            .unwrap()
            .unwrap();
        assert_eq!(
            record.time,
            crate::models::parse_time("2025-06-18 08:00").unwrap()
        );

        // 测试在时间点之后的最近温度
        let record = system
            .get_nearest_temp_record(1, "2025-06-18 09:00", false)
            .unwrap()
            .unwrap();
        assert_eq!(
            record.time,
            crate::models::parse_time("2025-06-18 10:00").unwrap()
        );
    }

    #[test]
    fn test_calculate_avg_temperature() {
        let (_temp_db, mut system) = setup_test_db();

        // 添加温度记录
        system
            .record_temperature(1, "2025-06-18 08:00", 100.0, 200.0)
            .unwrap();
        system
            .record_temperature(1, "2025-06-18 10:00", 200.0, 300.0)
            .unwrap();
        system
            .record_temperature(1, "2025-06-18 12:00", 300.0, 400.0)
            .unwrap();

        // 计算平均值
        let (avg_machine, avg_coke) = system
            .calculate_avg_temperature(1, "2025-06-18 09:00", "2025-06-18 11:00")
            .unwrap();

        // 预期平均值：梯形面积法计算
        // 09:00-10:00: (150+200)/2 = 175 (机侧), (250+300)/2 = 275 (焦侧)
        // 10:00-11:00: (200+250)/2 = 225 (机侧), (300+350)/2 = 325 (焦侧)
        // 平均：(175+225)/2 = 200 (机侧), (275+325)/2 = 300 (焦侧)
        assert!(
            (avg_machine - 200.0).abs() < 0.1,
            "机侧平均温度：{}",
            avg_machine
        );
        assert!((avg_coke - 300.0).abs() < 0.1, "焦侧平均温度：{}", avg_coke);
    }
}
