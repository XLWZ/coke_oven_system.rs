use chrono::{NaiveDate, NaiveDateTime};

// 温度记录点
pub struct TempRecord {
    pub time: NaiveDateTime,
    pub machine_side: f64,
    pub coke_side: f64,
}

// 时间-温度点
pub struct TimeTempPoint {
    pub time: NaiveDateTime,
    pub machine: f64,
    pub coke: f64,
}

// 时间格式解析器
pub fn parse_time(time_str: &str) -> Result<NaiveDateTime, String> {
    // 尝试带秒格式
    if let Ok(dt) = NaiveDateTime::parse_from_str(time_str, "%Y-%m-%d %H:%M:%S") {
        return Ok(dt);
    }

    // 再尝试不带秒的格式
    if let Ok(dt) = NaiveDateTime::parse_from_str(time_str, "%Y-%m-%d %H:%M") {
        return Ok(dt);
    }

    // 最后尝试仅日期格式
    if let Ok(date) = NaiveDate::parse_from_str(time_str, "%Y-%m-%d") {
        if let Some(dt) = date.and_hms_opt(0, 0, 0) {
            return Ok(dt);
        }
    }

    Err("无效时间格式".to_string())
}

// 辅助函数：根据前后两个记录插值指定时间点的温度
pub fn interpolate_temp(
    prev: &Option<TempRecord>,
    next: &Option<TempRecord>,
    target: NaiveDateTime,
) -> Option<(f64, f64)> {
    match (prev, next) {
        (Some(prev_rec), Some(next_rec)) => {
            let total_secs = (next_rec.time - prev_rec.time).num_seconds() as f64;
            if total_secs == 0.0 {
                return Some((prev_rec.machine_side, prev_rec.coke_side));
            }
            let secs_from_prev = (target - prev_rec.time).num_seconds() as f64;
            let ratio = secs_from_prev / total_secs;
            let machine =
                prev_rec.machine_side + (next_rec.machine_side - prev_rec.machine_side) * ratio;
            let coke = prev_rec.coke_side + (next_rec.coke_side - prev_rec.coke_side) * ratio;
            Some((machine, coke))
        }
        (Some(prev_res), None) => Some((prev_res.machine_side, prev_res.coke_side)),
        (None, Some(next_rec)) => Some((next_rec.machine_side, next_rec.coke_side)),
        (None, None) => None,
    }
}

// 测试代码
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};

    #[test]
    fn test_parse_time() {
        // 测试带秒的格式
        let time_str = "2025-06-18 08:16:30";
        let dt = parse_time(time_str).unwrap();
        assert_eq!(
            dt,
            NaiveDateTime::new(
                NaiveDate::from_ymd_opt(2025, 6, 18).unwrap(),
                NaiveTime::from_hms_opt(8, 16, 30).unwrap()
            )
        );

        // 测试不带秒的格式
        let time_str = "2025-06-18 08:16";
        let dt = parse_time(time_str).unwrap();
        assert_eq!(
            dt,
            NaiveDateTime::new(
                NaiveDate::from_ymd_opt(2025, 6, 18).unwrap(),
                NaiveTime::from_hms_opt(8, 16, 0).unwrap()
            )
        );

        // 测试仅日期格式
        let time_str = "2025-06-18";
        let dt = parse_time(time_str).unwrap();
        assert_eq!(
            dt,
            NaiveDateTime::new(
                NaiveDate::from_ymd_opt(2025, 6, 18).unwrap(),
                NaiveTime::from_hms_opt(0, 0, 0).unwrap()
            )
        );

        // 测试无效格式
        let time_str = "invalid-time";
        assert!(parse_time(time_str).is_err());
    }

    #[test]
    fn test_interpolate_temp() {
        let prev = Some(TempRecord {
            time: NaiveDateTime::new(
                NaiveDate::from_ymd_opt(2025, 6, 18).unwrap(),
                NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
            ),
            machine_side: 100.0,
            coke_side: 200.0,
        });

        let next = Some(TempRecord {
            time: NaiveDateTime::new(
                NaiveDate::from_ymd_opt(2025, 6, 18).unwrap(),
                NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            ),
            machine_side: 200.0,
            coke_side: 300.0,
        });

        let target = NaiveDateTime::new(
            NaiveDate::from_ymd_opt(2025, 6, 18).unwrap(),
            NaiveTime::from_hms_opt(8, 30, 0).unwrap(),
        );

        let result = interpolate_temp(&prev, &next, target).unwrap();
        assert_eq!(result, (150.0, 250.0));

        // 测试边界情况
        let result = interpolate_temp(&prev, &next, prev.as_ref().unwrap().time).unwrap();
        assert_eq!(result, (100.0, 200.0));

        let result = interpolate_temp(&prev, &next, next.as_ref().unwrap().time).unwrap();
        assert_eq!(result, (200.0, 300.0));
    }
}
