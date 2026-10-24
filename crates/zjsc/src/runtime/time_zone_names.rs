//! Os nomes longos em inglês dos fusos (`ucal_getTimeZoneDisplayName(UCAL_STANDARD ou UCAL_DST)` do ICU
//! com a língua padrão `en`), o campo entre parênteses do `Date.prototype.toString()` ("Brasilia Standard
//! Time"). O ICU os tira dos metafusos do CLDR; aqui há a tabela dos metafusos que cobrem os fusos do
//! mundo que o `TZ` de um sandbox costuma pedir.
//!
//! DIVERGÊNCIAS:
//!
//! - A tabela não é o CLDR inteiro: fuso que ela não lista cai no formato GMT localizado do ICU
//!   (`GMT-03:00`, `GMT` no deslocamento zero), que é o que o ICU devolve para fuso sem metafuso de nome.
//!   Fuso com nome no CLDR e ausente daqui sai com esse formato, diferente do Debian.
//! - O nome "de verão" (`UCAL_DST`) existe para todo metafuso, como no ICU, mesmo para fuso que hoje
//!   não tem horário de verão (`Brasilia Summer Time`).

use super::time_zone_names_data::ZONE_NAMES;

/// Um metafuso: os fusos IANA que o compartilham e os nomes padrão e de verão.
struct MetaZone {
    zones: &'static [&'static str],
    standard: &'static str,
    daylight: &'static str,
}

const META_ZONES: &[MetaZone] = &[
    MetaZone {
        zones: &["UTC", "Etc/UTC", "Etc/UCT", "Etc/Universal", "Etc/Zulu", "UCT", "Universal", "Zulu"],
        standard: "Coordinated Universal Time",
        daylight: "Coordinated Universal Time",
    },
    MetaZone {
        zones: &["GMT", "Etc/GMT", "Etc/Greenwich", "Etc/GMT0", "Etc/GMT+0", "Etc/GMT-0", "GMT0", "Greenwich"],
        standard: "Greenwich Mean Time",
        daylight: "Greenwich Mean Time",
    },
    MetaZone {
        zones: &[
            "America/Sao_Paulo", "Brazil/East", "America/Bahia", "America/Fortaleza", "America/Recife", "America/Maceio",
            "America/Araguaina", "America/Belem", "America/Santarem",
        ],
        standard: "Brasilia Standard Time",
        daylight: "Brasilia Summer Time",
    },
    MetaZone {
        zones: &["America/Manaus", "Brazil/West", "America/Porto_Velho", "America/Boa_Vista", "America/Cuiaba", "America/Campo_Grande"],
        standard: "Amazon Standard Time",
        daylight: "Amazon Summer Time",
    },
    MetaZone { zones: &["America/Rio_Branco", "Brazil/Acre", "America/Eirunepe"], standard: "Acre Standard Time", daylight: "Acre Summer Time" },
    MetaZone {
        zones: &["America/Noronha", "Brazil/DeNoronha"],
        standard: "Fernando de Noronha Standard Time",
        daylight: "Fernando de Noronha Summer Time",
    },
    MetaZone {
        zones: &[
            "America/Argentina/Buenos_Aires", "America/Buenos_Aires", "America/Argentina/Cordoba", "America/Cordoba",
            "America/Argentina/Mendoza", "America/Mendoza", "America/Argentina/Salta", "America/Argentina/Tucuman",
            "America/Argentina/Ushuaia", "America/Argentina/La_Rioja", "America/Argentina/San_Juan", "America/Argentina/San_Luis",
            "America/Argentina/Jujuy", "America/Jujuy", "America/Argentina/Catamarca", "America/Catamarca",
            "America/Argentina/Rio_Gallegos", "America/Argentina/ComodRivadavia",
        ],
        standard: "Argentina Standard Time",
        daylight: "Argentina Summer Time",
    },
    MetaZone { zones: &["America/Santiago", "Chile/Continental"], standard: "Chile Standard Time", daylight: "Chile Summer Time" },
    MetaZone { zones: &["America/Bogota"], standard: "Colombia Standard Time", daylight: "Colombia Summer Time" },
    MetaZone { zones: &["America/Lima"], standard: "Peru Standard Time", daylight: "Peru Summer Time" },
    MetaZone { zones: &["America/Caracas"], standard: "Venezuela Time", daylight: "Venezuela Time" },
    MetaZone { zones: &["America/Montevideo"], standard: "Uruguay Standard Time", daylight: "Uruguay Summer Time" },
    MetaZone { zones: &["America/La_Paz"], standard: "Bolivia Time", daylight: "Bolivia Time" },
    MetaZone { zones: &["America/Asuncion"], standard: "Paraguay Standard Time", daylight: "Paraguay Summer Time" },
    MetaZone { zones: &["America/Guayaquil"], standard: "Ecuador Time", daylight: "Ecuador Time" },
    MetaZone {
        zones: &[
            "America/New_York", "US/Eastern", "EST5EDT", "America/Detroit", "America/Toronto", "America/Montreal", "America/Nassau",
            "America/Indiana/Indianapolis", "America/Indianapolis", "America/Fort_Wayne", "America/Indiana/Marengo",
            "America/Indiana/Vevay", "America/Indiana/Vincennes", "America/Indiana/Petersburg", "America/Indiana/Winamac",
            "America/Kentucky/Louisville", "America/Louisville", "America/Kentucky/Monticello", "America/Iqaluit",
            "America/Panama", "America/Jamaica", "America/Cayman", "America/Cancun", "America/Port-au-Prince", "America/Grand_Turk",
            "EST", "America/Atikokan", "America/Coral_Harbour",
        ],
        standard: "Eastern Standard Time",
        daylight: "Eastern Daylight Time",
    },
    MetaZone {
        zones: &[
            "America/Chicago", "US/Central", "CST6CDT", "America/Winnipeg", "America/Menominee", "America/Indiana/Knox",
            "America/Indiana/Tell_City", "America/North_Dakota/Center", "America/North_Dakota/New_Salem",
            "America/North_Dakota/Beulah", "America/Mexico_City", "Mexico/General", "America/Merida", "America/Monterrey",
            "America/Matamoros", "America/Bahia_Banderas", "America/Costa_Rica", "America/Guatemala", "America/Belize",
            "America/El_Salvador", "America/Tegucigalpa", "America/Managua", "America/Regina", "America/Rainy_River",
            "America/Rankin_Inlet", "America/Resolute",
        ],
        standard: "Central Standard Time",
        daylight: "Central Daylight Time",
    },
    MetaZone {
        zones: &[
            "America/Denver", "US/Mountain", "MST7MDT", "America/Boise", "America/Edmonton", "America/Phoenix", "US/Arizona", "MST",
            "America/Ciudad_Juarez", "America/Yellowknife", "America/Cambridge_Bay", "America/Inuvik", "America/Chihuahua",
        ],
        standard: "Mountain Standard Time",
        daylight: "Mountain Daylight Time",
    },
    MetaZone {
        zones: &["America/Los_Angeles", "US/Pacific", "PST8PDT", "America/Vancouver", "America/Tijuana", "America/Ensenada"],
        standard: "Pacific Standard Time",
        daylight: "Pacific Daylight Time",
    },
    MetaZone {
        zones: &["America/Anchorage", "US/Alaska", "America/Juneau", "America/Sitka", "America/Nome", "America/Yakutat", "America/Metlakatla"],
        standard: "Alaska Standard Time",
        daylight: "Alaska Daylight Time",
    },
    MetaZone {
        zones: &["Pacific/Honolulu", "US/Hawaii", "HST", "America/Adak", "US/Aleutian"],
        standard: "Hawaii-Aleutian Standard Time",
        daylight: "Hawaii-Aleutian Daylight Time",
    },
    MetaZone {
        zones: &[
            "America/Halifax", "Canada/Atlantic", "America/Puerto_Rico", "America/Barbados", "America/Martinique", "America/Moncton",
            "America/Glace_Bay", "America/Thule", "Atlantic/Bermuda", "America/Santo_Domingo", "America/Port_of_Spain",
        ],
        standard: "Atlantic Standard Time",
        daylight: "Atlantic Daylight Time",
    },
    MetaZone { zones: &["America/St_Johns", "Canada/Newfoundland"], standard: "Newfoundland Standard Time", daylight: "Newfoundland Daylight Time" },
    MetaZone { zones: &["America/Havana", "Cuba"], standard: "Cuba Standard Time", daylight: "Cuba Daylight Time" },
    MetaZone {
        zones: &["Europe/London", "GB", "Europe/Belfast", "Europe/Jersey", "Europe/Guernsey", "Europe/Isle_of_Man"],
        standard: "Greenwich Mean Time",
        daylight: "British Summer Time",
    },
    MetaZone { zones: &["Europe/Dublin", "Eire"], standard: "Greenwich Mean Time", daylight: "Irish Standard Time" },
    MetaZone {
        zones: &[
            "Africa/Abidjan", "Africa/Accra", "Africa/Dakar", "Africa/Bamako", "Africa/Banjul", "Africa/Conakry", "Africa/Freetown",
            "Africa/Lome", "Africa/Monrovia", "Africa/Nouakchott", "Africa/Ouagadougou", "Atlantic/Reykjavik", "Iceland",
            "Atlantic/St_Helena", "America/Danmarkshavn",
        ],
        standard: "Greenwich Mean Time",
        daylight: "Greenwich Mean Time",
    },
    MetaZone {
        zones: &[
            "Europe/Lisbon", "Portugal", "Atlantic/Canary", "Atlantic/Madeira", "Atlantic/Faroe", "Atlantic/Faeroe", "WET",
            "Africa/Casablanca", "Africa/El_Aaiun",
        ],
        standard: "Western European Standard Time",
        daylight: "Western European Summer Time",
    },
    MetaZone {
        zones: &[
            "Europe/Paris", "Europe/Berlin", "Europe/Madrid", "Europe/Rome", "Europe/Amsterdam", "Europe/Brussels", "Europe/Vienna",
            "Europe/Zurich", "Europe/Stockholm", "Europe/Oslo", "Europe/Copenhagen", "Europe/Prague", "Europe/Warsaw", "Poland",
            "Europe/Budapest", "Europe/Belgrade", "Europe/Zagreb", "Europe/Luxembourg", "Europe/Monaco", "Europe/Malta",
            "Europe/Andorra", "Europe/Tirane", "Europe/Sarajevo", "Europe/Skopje", "Europe/Ljubljana", "Europe/Bratislava",
            "Europe/Vaduz", "Europe/San_Marino", "Europe/Vatican", "Europe/Gibraltar", "Europe/Podgorica", "Europe/Busingen",
            "Arctic/Longyearbyen", "Atlantic/Jan_Mayen", "Africa/Algiers", "Africa/Tunis", "Africa/Ceuta", "CET", "MET",
        ],
        standard: "Central European Standard Time",
        daylight: "Central European Summer Time",
    },
    MetaZone {
        zones: &[
            "Europe/Athens", "Europe/Helsinki", "Europe/Kiev", "Europe/Kyiv", "Europe/Bucharest", "Europe/Sofia", "Europe/Riga",
            "Europe/Tallinn", "Europe/Vilnius", "Europe/Chisinau", "Europe/Mariehamn", "Europe/Uzhgorod", "Europe/Zaporozhye",
            "Asia/Nicosia", "Europe/Nicosia", "Asia/Beirut", "Africa/Cairo", "Egypt", "Europe/Kaliningrad", "Africa/Tripoli",
            "Libya", "Asia/Famagusta", "EET",
        ],
        standard: "Eastern European Standard Time",
        daylight: "Eastern European Summer Time",
    },
    MetaZone { zones: &["Europe/Moscow", "W-SU", "Europe/Simferopol", "Europe/Kirov"], standard: "Moscow Standard Time", daylight: "Moscow Summer Time" },
    MetaZone { zones: &["Europe/Minsk"], standard: "Moscow Standard Time", daylight: "Moscow Summer Time" },
    MetaZone { zones: &["Asia/Jerusalem", "Asia/Tel_Aviv", "Israel"], standard: "Israel Standard Time", daylight: "Israel Daylight Time" },
    MetaZone {
        zones: &["Asia/Riyadh", "Asia/Baghdad", "Asia/Kuwait", "Asia/Qatar", "Asia/Bahrain", "Asia/Aden"],
        standard: "Arabian Standard Time",
        daylight: "Arabian Daylight Time",
    },
    MetaZone { zones: &["Asia/Dubai", "Asia/Muscat"], standard: "Gulf Standard Time", daylight: "Gulf Standard Time" },
    MetaZone { zones: &["Asia/Tehran", "Iran"], standard: "Iran Standard Time", daylight: "Iran Daylight Time" },
    MetaZone { zones: &["Asia/Karachi"], standard: "Pakistan Standard Time", daylight: "Pakistan Summer Time" },
    MetaZone { zones: &["Asia/Kolkata", "Asia/Calcutta"], standard: "India Standard Time", daylight: "India Standard Time" },
    MetaZone { zones: &["Asia/Colombo"], standard: "India Standard Time", daylight: "India Standard Time" },
    MetaZone { zones: &["Asia/Kathmandu", "Asia/Katmandu"], standard: "Nepal Time", daylight: "Nepal Time" },
    MetaZone { zones: &["Asia/Dhaka", "Asia/Dacca"], standard: "Bangladesh Standard Time", daylight: "Bangladesh Summer Time" },
    MetaZone {
        zones: &["Asia/Bangkok", "Asia/Ho_Chi_Minh", "Asia/Saigon", "Asia/Phnom_Penh", "Asia/Vientiane"],
        standard: "Indochina Time",
        daylight: "Indochina Time",
    },
    MetaZone { zones: &["Asia/Singapore", "Singapore", "Asia/Kuala_Lumpur"], standard: "Singapore Standard Time", daylight: "Singapore Standard Time" },
    MetaZone { zones: &["Asia/Jakarta"], standard: "Western Indonesia Time", daylight: "Western Indonesia Time" },
    MetaZone { zones: &["Asia/Manila"], standard: "Philippine Standard Time", daylight: "Philippine Summer Time" },
    MetaZone { zones: &["Asia/Shanghai", "PRC", "Asia/Chongqing", "Asia/Harbin", "Asia/Macau"], standard: "China Standard Time", daylight: "China Daylight Time" },
    MetaZone { zones: &["Asia/Hong_Kong", "Hongkong"], standard: "Hong Kong Standard Time", daylight: "Hong Kong Summer Time" },
    MetaZone { zones: &["Asia/Taipei", "ROC"], standard: "Taipei Standard Time", daylight: "Taipei Daylight Time" },
    MetaZone { zones: &["Asia/Tokyo", "Japan"], standard: "Japan Standard Time", daylight: "Japan Daylight Time" },
    MetaZone { zones: &["Asia/Seoul", "ROK"], standard: "Korean Standard Time", daylight: "Korean Daylight Time" },
    MetaZone {
        zones: &[
            "Australia/Sydney", "Australia/Melbourne", "Australia/Hobart", "Australia/Brisbane", "Australia/Lindeman",
            "Australia/Canberra", "Australia/ACT", "Australia/NSW", "Australia/Victoria", "Australia/Queensland",
            "Australia/Tasmania", "Australia/Currie",
        ],
        standard: "Australian Eastern Standard Time",
        daylight: "Australian Eastern Daylight Time",
    },
    MetaZone {
        zones: &["Australia/Adelaide", "Australia/Darwin", "Australia/South", "Australia/North", "Australia/Broken_Hill", "Australia/Yancowinna"],
        standard: "Australian Central Standard Time",
        daylight: "Australian Central Daylight Time",
    },
    MetaZone { zones: &["Australia/Perth", "Australia/West"], standard: "Australian Western Standard Time", daylight: "Australian Western Daylight Time" },
    MetaZone { zones: &["Pacific/Auckland", "NZ", "Antarctica/McMurdo"], standard: "New Zealand Standard Time", daylight: "New Zealand Daylight Time" },
    MetaZone { zones: &["Africa/Johannesburg", "Africa/Maseru", "Africa/Mbabane"], standard: "South Africa Standard Time", daylight: "South Africa Standard Time" },
    MetaZone {
        zones: &[
            "Africa/Maputo", "Africa/Harare", "Africa/Lusaka", "Africa/Kigali", "Africa/Gaborone", "Africa/Blantyre", "Africa/Bujumbura",
            "Africa/Lubumbashi", "Africa/Windhoek", "Africa/Khartoum", "Africa/Juba",
        ],
        standard: "Central Africa Time",
        daylight: "Central Africa Time",
    },
    MetaZone {
        zones: &[
            "Africa/Nairobi", "Africa/Addis_Ababa", "Africa/Dar_es_Salaam", "Africa/Kampala", "Africa/Mogadishu", "Africa/Djibouti",
            "Africa/Asmara", "Indian/Antananarivo", "Indian/Comoro", "Indian/Mayotte",
        ],
        standard: "East Africa Time",
        daylight: "East Africa Time",
    },
    MetaZone {
        zones: &[
            "Africa/Lagos", "Africa/Kinshasa", "Africa/Luanda", "Africa/Douala", "Africa/Libreville", "Africa/Brazzaville",
            "Africa/Bangui", "Africa/Malabo", "Africa/Niamey", "Africa/Ndjamena", "Africa/Porto-Novo",
        ],
        standard: "West Africa Standard Time",
        daylight: "West Africa Summer Time",
    },
];

/// Metafuso do fuso IANA, ou `None`.
fn meta_zone(iana: &str) -> Option<&'static MetaZone> {
    META_ZONES.iter().find(|meta| meta.zones.contains(&iana))
}

/// O nome longo do fuso IANA (`UCAL_STANDARD` ou `UCAL_DST`), ou `None` quando não há nome. A tabela
/// medida no bun (`time_zone_names_data`) manda: entrada com nome padrão vazio é fuso sem nome no ICU
/// (`None`, o chamador usa o GMT localizado); nome de verão vazio cai nos metafusos acima e, sem eles, no
/// nome padrão. Zona fora da tabela medida usa só os metafusos.
pub fn long_name(iana: &str, is_dst: bool) -> Option<&'static str> {
    let measured = ZONE_NAMES.binary_search_by(|(zone, _, _)| (*zone).cmp(iana)).ok().map(|index| ZONE_NAMES[index]);
    let Some((_, standard, daylight)) = measured else {
        return meta_zone(iana).map(|meta| if is_dst { meta.daylight } else { meta.standard });
    };
    if standard.is_empty() {
        return None;
    }
    if !is_dst {
        return Some(standard);
    }
    if !daylight.is_empty() {
        return Some(daylight);
    }
    // Metafuso cujo nome de verão repete o padrão (India, Gulf, Singapore) não tem nome de verão no
    // CLDR: o ICU cai no GMT localizado (`Mon Jan 01 1945 ... (GMT+05:30)` em Kolkata, em +0630).
    match meta_zone(iana) {
        Some(meta) if meta.daylight == meta.standard => None,
        Some(meta) => Some(meta.daylight),
        None => Some(standard),
    }
}

/// O formato GMT localizado do ICU para `offset_seconds` (`GMT-03:00`; `GMT` no deslocamento zero).
pub fn localized_gmt(offset_seconds: i32) -> String {
    if offset_seconds == 0 {
        return "GMT".to_string();
    }
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let minutes_total = offset_seconds.abs() / 60;
    format!("GMT{sign}{:02}:{:02}", minutes_total / 60, minutes_total % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brasilia_names() {
        assert_eq!(long_name("America/Sao_Paulo", false), Some("Brasilia Standard Time"));
        assert_eq!(long_name("America/Sao_Paulo", true), Some("Brasilia Summer Time"));
        assert_eq!(long_name("Nowhere/Land", false), None);
    }

    #[test]
    fn localized_gmt_format() {
        assert_eq!(localized_gmt(-10800), "GMT-03:00");
        assert_eq!(localized_gmt(19800), "GMT+05:30");
        assert_eq!(localized_gmt(0), "GMT");
    }
}
