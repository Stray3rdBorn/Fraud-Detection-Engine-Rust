// earth radius in kilometers, used by the distance formula.
const EARTH_RADIUS_KM: f64 = 6371.0;

// cities as name, latitude and longitude, United States and Bangladesh.
const CITIES: &[(&str, f64, f64)] = &[
    // United States cities, the cities used in the bank transaction data.
    ("Albuquerque", 35.0844, -106.6504),
    ("Atlanta", 33.7490, -84.3880),
    ("Austin", 30.2672, -97.7431),
    ("Baltimore", 39.2904, -76.6122),
    ("Boston", 42.3601, -71.0589),
    ("Charlotte", 35.2271, -80.8431),
    ("Chicago", 41.8781, -87.6298),
    ("Colorado Springs", 38.8339, -104.8214),
    ("Columbus", 39.9612, -82.9988),
    ("Dallas", 32.7767, -96.7970),
    ("Denver", 39.7392, -104.9903),
    ("Detroit", 42.3314, -83.0458),
    ("El Paso", 31.7619, -106.4850),
    ("Fort Worth", 32.7555, -97.3308),
    ("Fresno", 36.7378, -119.7871),
    ("Houston", 29.7604, -95.3698),
    ("Indianapolis", 39.7684, -86.1581),
    ("Jacksonville", 30.3322, -81.6557),
    ("Kansas City", 39.0997, -94.5786),
    ("Las Vegas", 36.1699, -115.1398),
    ("Los Angeles", 34.0522, -118.2437),
    ("Louisville", 38.2527, -85.7585),
    ("Memphis", 35.1495, -90.0490),
    ("Mesa", 33.4152, -111.8315),
    ("Miami", 25.7617, -80.1918),
    ("Milwaukee", 43.0389, -87.9065),
    ("Nashville", 36.1627, -86.7816),
    ("New York", 40.7128, -74.0060),
    ("Oklahoma City", 35.4676, -97.5164),
    ("Omaha", 41.2565, -95.9345),
    ("Philadelphia", 39.9526, -75.1652),
    ("Phoenix", 33.4484, -112.0740),
    ("Portland", 45.5152, -122.6784),
    ("Raleigh", 35.7796, -78.6382),
    ("Sacramento", 38.5816, -121.4944),
    ("San Antonio", 29.4241, -98.4936),
    ("San Diego", 32.7157, -117.1611),
    ("San Francisco", 37.7749, -122.4194),
    ("San Jose", 37.3382, -121.8863),
    ("Seattle", 47.6062, -122.3321),
    ("Tucson", 32.2226, -110.9747),
    ("Virginia Beach", 36.8529, -75.9780),
    ("Washington", 38.9072, -77.0369),
    // Bangladesh, all 64 district headquarters plus other large towns, for internal fraud checks.
    // Dhaka division.
    ("Dhaka", 23.8103, 90.4125),
    ("Gazipur", 23.9999, 90.4203),
    ("Narayanganj", 23.6238, 90.5000),
    ("Narsingdi", 23.9322, 90.7151),
    ("Manikganj", 23.8617, 90.0003),
    ("Munshiganj", 23.5422, 90.5305),
    ("Faridpur", 23.6070, 89.8429),
    ("Gopalganj", 23.0050, 89.8266),
    ("Madaripur", 23.1641, 90.1897),
    ("Rajbari", 23.7574, 89.6445),
    ("Shariatpur", 23.2423, 90.4348),
    ("Tangail", 24.2513, 89.9167),
    ("Kishoreganj", 24.4449, 90.7766),
    // Chattogram division.
    ("Chattogram", 22.3569, 91.7832),
    ("Cox's Bazar", 21.4272, 92.0058),
    ("Cumilla", 23.4607, 91.1809),
    ("Feni", 23.0159, 91.3976),
    ("Brahmanbaria", 23.9571, 91.1119),
    ("Chandpur", 23.2333, 90.6713),
    ("Lakshmipur", 22.9425, 90.8412),
    ("Noakhali", 22.8696, 91.0995),
    ("Khagrachhari", 23.1193, 91.9847),
    ("Rangamati", 22.7324, 92.2985),
    ("Bandarban", 22.1953, 92.2184),
    // Rajshahi division.
    ("Rajshahi", 24.3745, 88.6042),
    ("Bogura", 24.8465, 89.3776),
    ("Joypurhat", 25.0968, 89.0227),
    ("Naogaon", 24.7936, 88.9318),
    ("Natore", 24.4206, 89.0000),
    ("Chapainawabganj", 24.5965, 88.2775),
    ("Pabna", 24.0064, 89.2372),
    ("Sirajganj", 24.4534, 89.7006),
    // Khulna division.
    ("Khulna", 22.8456, 89.5403),
    ("Bagerhat", 22.6602, 89.7895),
    ("Chuadanga", 23.6401, 88.8418),
    ("Jashore", 23.1667, 89.2167),
    ("Jhenaidah", 23.5448, 89.1539),
    ("Kushtia", 23.9013, 89.1200),
    ("Magura", 23.4873, 89.4199),
    ("Meherpur", 23.7622, 88.6318),
    ("Narail", 23.1725, 89.5126),
    ("Satkhira", 22.7185, 89.0705),
    // Barishal division.
    ("Barishal", 22.7010, 90.3535),
    ("Barguna", 22.1591, 90.1257),
    ("Bhola", 22.6859, 90.6482),
    ("Jhalokati", 22.6406, 90.1987),
    ("Patuakhali", 22.3596, 90.3299),
    ("Pirojpur", 22.5841, 89.9720),
    // Sylhet division.
    ("Sylhet", 24.8949, 91.8687),
    ("Habiganj", 24.3745, 91.4156),
    ("Moulvibazar", 24.4829, 91.7774),
    ("Sunamganj", 25.0658, 91.3950),
    // Rangpur division.
    ("Rangpur", 25.7439, 89.2752),
    ("Dinajpur", 25.6279, 88.6332),
    ("Gaibandha", 25.3297, 89.5280),
    ("Kurigram", 25.8054, 89.6361),
    ("Lalmonirhat", 25.9923, 89.2847),
    ("Nilphamari", 25.9318, 88.8560),
    ("Panchagarh", 26.3411, 88.5541),
    ("Thakurgaon", 26.0337, 88.4617),
    // Mymensingh division.
    ("Mymensingh", 24.7471, 90.4203),
    ("Jamalpur", 24.9375, 89.9372),
    ("Netrokona", 24.8703, 90.7279),
    ("Sherpur", 25.0204, 90.0152),
    // other large towns and border or port towns.
    ("Savar", 23.8583, 90.2667),
    ("Tongi", 23.8918, 90.4090),
    ("Bhairab", 24.0525, 90.9761),
    ("Sonargaon", 23.6500, 90.6000),
    ("Sitakunda", 22.6150, 91.6670),
    ("Kaptai", 22.4986, 92.2133),
    ("Teknaf", 20.8624, 92.3058),
    ("Kuakata", 21.8133, 90.1190),
    ("Sreemangal", 24.3065, 91.7296),
    ("Benapole", 23.0449, 88.8909),
    ("Mongla", 22.4750, 89.6000),
    ("Saidpur", 25.7795, 88.8910),
    ("Ishwardi", 24.1333, 89.0667),
];

// other spellings that people and datasets use, each one points to the name in the table above.
const ALIASES: &[(&str, &str)] = &[
    ("Dacca", "Dhaka"),
    ("Chittagong", "Chattogram"),
    ("Coxs Bazar", "Cox's Bazar"),
    ("Cox Bazar", "Cox's Bazar"),
    ("Comilla", "Cumilla"),
    ("Jessore", "Jashore"),
    ("Bogra", "Bogura"),
    ("Barisal", "Barishal"),
    ("Chapai Nawabganj", "Chapainawabganj"),
    ("Maulvibazar", "Moulvibazar"),
    ("Khagrachari", "Khagrachhari"),
    ("Jhalakathi", "Jhalokati"),
    ("Netrakona", "Netrokona"),
    ("Jaipurhat", "Joypurhat"),
];

// finds the coordinates of a city, the match ignores upper and lower case and accepts other spellings.
pub fn lookup(name: &str) -> Option<(f64, f64)> {
    // an other spelling is replaced by the name used in the table, any other name stays as it is.
    let name = ALIASES
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(name))
        .map_or(name, |&(_, current)| current);
    // search the table for a matching name.
    CITIES
        .iter()
        .find(|(city, _, _)| city.eq_ignore_ascii_case(name))
        // keep only the coordinates of the match.
        .map(|&(_, lat, lon)| (lat, lon))
}

// distance in kilometers between two points on the earth, using the haversine formula.
pub fn distance_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    // both latitudes in radians.
    let (lat1, lat2) = (a.0.to_radians(), b.0.to_radians());
    // difference in latitude in radians.
    let dlat = lat2 - lat1;
    // difference in longitude in radians.
    let dlon = (b.1 - a.1).to_radians();
    // haversine of the angle between the two points, seen from the earth's center.
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    // the angle times the radius gives the distance along the surface, min guards against rounding above 1.
    2.0 * EARTH_RADIUS_KM * h.sqrt().min(1.0).asin()
}

// tests for the geo helpers.
#[cfg(test)]
mod tests {
    // bring the functions above into the tests.
    use super::*;

    // known cities are found in any letter case and unknown ones are not.
    #[test]
    fn lookup_ignores_case_and_rejects_unknown_cities() {
        // lower case input still finds the city.
        assert_eq!(lookup("new york"), Some((40.7128, -74.0060)));
        // a name that is not in the table gives nothing.
        assert_eq!(lookup("Nowhere"), None);
    }

    // the distance between two far cities is close to the real value.
    #[test]
    fn distance_new_york_to_los_angeles_is_about_3936_km() {
        // coordinates of both cities from the table.
        let ny = lookup("New York").unwrap();
        // second city.
        let la = lookup("Los Angeles").unwrap();
        // the real distance is about 3936 km, a small tolerance covers rounding.
        assert!((distance_km(ny, la) - 3936.0).abs() < 30.0);
        // the distance from a point to itself is zero.
        assert_eq!(distance_km(ny, ny), 0.0);
    }

    // Bangladesh cities are found, also under other spellings, and the distance is close to the real value.
    #[test]
    fn bangladesh_cities_and_other_spellings_work() {
        // the old name of Chattogram finds the same place as the current name.
        assert_eq!(lookup("Chittagong"), lookup("Chattogram"));
        // coordinates of both cities from the table.
        let dhaka = lookup("Dhaka").unwrap();
        // second city.
        let chattogram = lookup("Chattogram").unwrap();
        // the straight line distance is about 214 km, a small tolerance covers rounding.
        assert!((distance_km(dhaka, chattogram) - 214.0).abs() < 10.0);
    }

    // the table has no typos that put a city outside both countries, no repeated names and no broken aliases.
    #[test]
    fn table_is_consistent() {
        // every city must lie inside the United States or inside Bangladesh.
        for &(name, lat, lon) in CITIES {
            // rough box around the United States.
            let in_us = (25.0..=48.0).contains(&lat) && (-125.0..=-66.0).contains(&lon);
            // rough box around Bangladesh.
            let in_bd = (20.5..=26.7).contains(&lat) && (88.0..=92.7).contains(&lon);
            // one of the two must hold.
            assert!(in_us || in_bd, "{name} is outside both countries");
        }
        // every name appears once.
        for (index, (name, _, _)) in CITIES.iter().enumerate() {
            // no earlier row has the same name.
            assert!(
                !CITIES[..index]
                    .iter()
                    .any(|(other, _, _)| other.eq_ignore_ascii_case(name)),
                "{name} repeated"
            );
        }
        // every alias leads to a name in the table.
        for &(alias, current) in ALIASES {
            // the target exists.
            assert!(CITIES.iter().any(|(city, _, _)| *city == current), "{alias}");
        }
    }
}