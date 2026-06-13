import Foundation

public enum NumberToWords {
    private static let units: [Int: String] = [
        0: "zero",
        1: "one",
        2: "two",
        3: "three",
        4: "four",
        5: "five",
        6: "six",
        7: "seven",
        8: "eight",
        9: "nine",
        10: "ten",
        11: "eleven",
        12: "twelve",
        13: "thirteen",
        14: "fourteen",
        15: "fifteen",
        16: "sixteen",
        17: "seventeen",
        18: "eighteen",
        19: "nineteen",
    ]

    private static let tens: [Int: String] = [
        20: "twenty",
        30: "thirty",
        40: "forty",
        50: "fifty",
        60: "sixty",
        70: "seventy",
        80: "eighty",
        90: "ninety",
    ]

    private static let ordinalUnits: [Int: String] = [
        0: "zeroth",
        1: "first",
        2: "second",
        3: "third",
        4: "fourth",
        5: "fifth",
        6: "sixth",
        7: "seventh",
        8: "eighth",
        9: "ninth",
        10: "tenth",
        11: "eleventh",
        12: "twelfth",
        13: "thirteenth",
        14: "fourteenth",
        15: "fifteenth",
        16: "sixteenth",
        17: "seventeenth",
        18: "eighteenth",
        19: "nineteenth",
    ]

    private static let ordinalTens: [Int: String] = [
        20: "twentieth",
        30: "thirtieth",
        40: "fortieth",
        50: "fiftieth",
        60: "sixtieth",
        70: "seventieth",
        80: "eightieth",
        90: "ninetieth",
    ]

    private static let scales: [(UInt64, String)] = [
        (1_000_000_000_000_000_000, "quintillion"),
        (1_000_000_000_000_000, "quadrillion"),
        (1_000_000_000_000, "trillion"),
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ]

    public static func cardinal(_ value: Int64) -> String {
        if value < 0 {
            if value == .min {
                return "minus " + cardinalMagnitude(value.magnitude)
            }
            return "minus " + cardinalMagnitude(UInt64(-value))
        }
        return cardinalMagnitude(UInt64(value))
    }

    public static func ordinal(_ value: Int64) -> String {
        if value < 0 {
            if value == .min {
                return "minus " + ordinalMagnitude(value.magnitude)
            }
            return "minus " + ordinalMagnitude(UInt64(-value))
        }
        return ordinalMagnitude(UInt64(value))
    }

    public static func year(_ value: Int64) -> String {
        if value < 0 {
            if value == .min {
                return "minus " + yearMagnitude(value.magnitude)
            }
            return "minus " + yearMagnitude(UInt64(-value))
        }
        return yearMagnitude(UInt64(value))
    }

    private static func cardinalMagnitude(_ value: UInt64) -> String {
        if let unit = units[Int(value)] {
            return unit
        }
        if value < 100 {
            let tensValue = Int(value / 10) * 10
            let remainder = Int(value % 10)
            guard let tensWord = tens[tensValue] else { return String(value) }
            return remainder == 0 ? tensWord : "\(tensWord)-\(units[remainder]!)"
        }
        if value < 1_000 {
            let hundreds = value / 100
            let remainder = value % 100
            let base = "\(units[Int(hundreds)]!) hundred"
            return remainder == 0 ? base : "\(base) \(cardinalMagnitude(remainder))"
        }
        for (scaleValue, scaleName) in scales where value >= scaleValue {
            let major = value / scaleValue
            let remainder = value % scaleValue
            let base = "\(cardinalMagnitude(major)) \(scaleName)"
            return remainder == 0 ? base : "\(base) \(cardinalMagnitude(remainder))"
        }
        return String(value)
    }

    private static func ordinalMagnitude(_ value: UInt64) -> String {
        if let direct = ordinalUnits[Int(value)] {
            return direct
        }
        if value < 100 {
            let tensValue = Int(value / 10) * 10
            let remainder = Int(value % 10)
            guard let tensWord = tens[tensValue] else { return String(value) }
            return remainder == 0 ? (ordinalTens[tensValue] ?? "\(tensWord)th") : "\(tensWord)-\(ordinalUnits[remainder]!)"
        }
        if value < 1_000 {
            let hundreds = value / 100
            let remainder = value % 100
            let base = "\(units[Int(hundreds)]!) hundred"
            return remainder == 0 ? "\(base)th" : "\(base) \(ordinalMagnitude(remainder))"
        }
        for (scaleValue, scaleName) in scales where value >= scaleValue {
            let major = value / scaleValue
            let remainder = value % scaleValue
            let base = "\(cardinalMagnitude(major)) \(scaleName)"
            return remainder == 0 ? "\(base)th" : "\(base) \(ordinalMagnitude(remainder))"
        }
        return "\(value)th"
    }

    private static func yearMagnitude(_ value: UInt64) -> String {
        guard (1000...9999).contains(value) else {
            return cardinalMagnitude(value)
        }

        let high = value / 100
        let low = value % 100

        if value >= 2000 && value <= 2009 {
            return low == 0 ? "two thousand" : "two thousand \(cardinalMagnitude(low))"
        }

        if low == 0 {
            return "\(cardinalMagnitude(high)) hundred"
        }

        if low < 10 {
            return "\(cardinalMagnitude(high)) oh-\(cardinalMagnitude(low))"
        }

        return "\(cardinalMagnitude(high)) \(cardinalMagnitude(low))"
    }

    public static func cardinal(_ text: String) -> String? {
        Int64(text).map(cardinal)
    }

    public static func ordinal(_ text: String) -> String? {
        Int64(text).map(ordinal)
    }

    public static func year(_ text: String) -> String? {
        Int64(text).map(year)
    }
}
