import Foundation

public struct ProsodyState: Sendable, Equatable {
    public var rate: Float = 1.0
    public var pitch: Float = 0.0
    
    public init(rate: Float = 1.0, pitch: Float = 0.0) {
        self.rate = rate
        self.pitch = pitch
    }
}

public struct ParsedMarkup: Sendable {
    public let cleanText: String
    /// Character-level prosody states matching cleanText 1-to-1
    public let characterProsody: [ProsodyState]
}

public enum ProsodyMarkupParser {
    
    public static func parse(_ input: String) -> ParsedMarkup {
        var cleanText = ""
        var characterProsody: [ProsodyState] = []
        
        var stack: [ProsodyState] = [ProsodyState()]
        var cursor = input.startIndex
        
        while cursor < input.endIndex {
            if input[cursor] == "<" {
                // Potential tag
                if let tagEndIndex = input[cursor...].firstIndex(of: ">") {
                    let tagContent = String(input[input.index(after: cursor)..<tagEndIndex]).trimmingCharacters(in: .whitespacesAndNewlines)
                    cursor = input.index(after: tagEndIndex)
                    
                    if tagContent.hasPrefix("/") {
                        // Closing tag
                        if stack.count > 1 {
                            stack.removeLast()
                        }
                    } else {
                        // Opening tag, e.g. prosody rate="1.5" pitch="+20Hz"
                        let parts = tagContent.split(separator: " ", maxSplits: 1).map(String.init)
                        let tagName = parts[0].lowercased()
                        
                        if tagName == "prosody" && parts.count > 1 {
                            var newState = stack.last ?? ProsodyState()
                            parseAttributes(parts[1], into: &newState)
                            stack.append(newState)
                        } else {
                            // Unsupported tag, push duplicate state to keep stack balanced
                            stack.append(stack.last ?? ProsodyState())
                        }
                    }
                    continue
                }
            }
            
            // Normal character
            let char = input[cursor]
            cleanText.append(char)
            characterProsody.append(stack.last ?? ProsodyState())
            cursor = input.index(after: cursor)
        }
        
        return ParsedMarkup(cleanText: cleanText, characterProsody: characterProsody)
    }
    
    private static func parseAttributes(_ attrString: String, into state: inout ProsodyState) {
        // Parse attributes like: rate="1.5" pitch="+20Hz"
        let regex = try? NSRegularExpression(pattern: "(\\w+)\\s*=\\s*\"([^\"]+)\"", options: [])
        let range = NSRange(attrString.startIndex..<attrString.endIndex, in: attrString)
        
        regex?.enumerateMatches(in: attrString, options: [], range: range) { match, _, _ in
            guard let match = match, match.numberOfRanges == 3 else { return }
            guard let keyRange = Range(match.range(at: 1), in: attrString),
                  let valRange = Range(match.range(at: 2), in: attrString) else { return }
            
            let key = String(attrString[keyRange]).lowercased()
            let val = String(attrString[valRange])
            
            if key == "rate" {
                if val.hasSuffix("%"), let percentVal = Float(val.dropLast()) {
                    state.rate = percentVal / 100.0
                } else if let rateVal = Float(val) {
                    state.rate = rateVal
                }
            } else if key == "pitch" {
                // Supports +20Hz, -10Hz, or float values
                var cleanVal = val.lowercased()
                if cleanVal.hasSuffix("hz") {
                    cleanVal = String(cleanVal.dropLast(2))
                }
                if let pitchVal = Float(cleanVal) {
                    state.pitch = pitchVal
                }
            }
        }
    }
}
