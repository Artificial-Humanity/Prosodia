import Foundation

public final class MToken: CustomStringConvertible {
    public final class Underscore {
        public var isHead: Bool
        public var alias: String?
        public var stress: Double?
        public var currency: String?
        public var numFlags: String
        public var prespace: Bool
        public var rating: Int?

        public init(
            isHead: Bool = true,
            alias: String? = nil,
            stress: Double? = nil,
            currency: String? = nil,
            numFlags: String = "",
            prespace: Bool = false,
            rating: Int? = nil
        ) {
            self.isHead = isHead
            self.alias = alias
            self.stress = stress
            self.currency = currency
            self.numFlags = numFlags
            self.prespace = prespace
            self.rating = rating
        }

        public func cloned() -> Underscore {
            Underscore(
                isHead: isHead,
                alias: alias,
                stress: stress,
                currency: currency,
                numFlags: numFlags,
                prespace: prespace,
                rating: rating
            )
        }
    }

    public var text: String
    public var tag: String
    public var whitespace: String
    public var phonemes: String?
    public var startTS: Double?
    public var endTS: Double?
    public var `_`: Underscore?

    public var underscore: Underscore? {
        get { `_` }
        set { `_` = newValue }
    }

    public var rating: Int? {
        get { underscore?.rating }
        set { underscore?.rating = newValue }
    }

    public init(
        text: String,
        tag: String,
        whitespace: String,
        phonemes: String? = nil,
        startTS: Double? = nil,
        endTS: Double? = nil,
        underscore: Underscore? = nil
    ) {
        self.text = text
        self.tag = tag
        self.whitespace = whitespace
        self.phonemes = phonemes
        self.startTS = startTS
        self.endTS = endTS
        self._ = underscore
    }

    public func cloned() -> MToken {
        MToken(
            text: text,
            tag: tag,
            whitespace: whitespace,
            phonemes: phonemes,
            startTS: startTS,
            endTS: endTS,
            underscore: underscore?.cloned()
        )
    }

    public var description: String {
        "MToken(text: \(text.debugDescription), tag: \(tag.debugDescription), whitespace: \(whitespace.debugDescription), phonemes: \(String(describing: phonemes)), rating: \(String(describing: rating)))"
    }
}
