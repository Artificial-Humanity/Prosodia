import Foundation

// MARK: - XML Helper Parsers

public final class ContainerXMLParser: NSObject, XMLParserDelegate {
    public var opfPath: String?
    
    public override init() {
        super.init()
    }
    
    public func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?, qualifiedName qName: String?, attributes attributeDict: [String : String] = [:]) {
        let localName = elementName.components(separatedBy: ":").last ?? elementName
        if localName == "rootfile" {
            opfPath = attributeDict["full-path"]
        }
    }
}

public final class OPFXMLParser: NSObject, XMLParserDelegate {
    public var hrefs: [String: String] = [:]
    public var spineItemRefs: [String] = []
    public var mediaTypes: [String: String] = [:]
    
    // Metadata properties
    public var title: String?
    public var author: String?
    public var language: String?
    public var publisher: String?
    public var date: String?
    public var identifier: String?
    public var bookDescription: String?
    public var subjects: [String] = []
    
    // TOC and Navigation resolutions
    public var spineTOCId: String?
    public var navHref: String?
    
    public var ncxHref: String? {
        guard let tocId = spineTOCId else { return nil }
        return hrefs[tocId]
    }
    
    private var coverMetaId: String?
    private var epub3CoverHref: String?
    
    public var resolvedCoverHref: String? {
        if let epub3 = epub3CoverHref {
            return epub3
        }
        if let metaId = coverMetaId {
            return hrefs[metaId]
        }
        return nil
    }
    
    private var currentElement: String?
    private var tempCharacters: String = ""
    
    public override init() {
        super.init()
    }
    
    public func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?, qualifiedName qName: String?, attributes attributeDict: [String : String] = [:]) {
        let localName = elementName.components(separatedBy: ":").last ?? elementName
        currentElement = localName
        tempCharacters = ""
        
        if localName == "item" {
            if let id = attributeDict["id"], let href = attributeDict["href"] {
                hrefs[id] = href
                if let mediaType = attributeDict["media-type"] {
                    mediaTypes[id] = mediaType
                }
                
                // EPUB 3 cover image declaration
                if let properties = attributeDict["properties"], properties.contains("cover-image") {
                    epub3CoverHref = href
                }
                // EPUB 3 TOC navigation document
                if let properties = attributeDict["properties"], properties.contains("nav") {
                    navHref = href
                }
            }
        } else if localName == "itemref" {
            if let idref = attributeDict["idref"] {
                if attributeDict["linear"] != "no" {
                    spineItemRefs.append(idref)
                }
            }
        } else if localName == "spine" {
            spineTOCId = attributeDict["toc"]
        } else if localName == "meta" {
            // EPUB 2 cover image reference
            if attributeDict["name"] == "cover" {
                coverMetaId = attributeDict["content"]
            }
        }
    }
    
    public func parser(_ parser: XMLParser, foundCharacters string: String) {
        if currentElement == "title" || currentElement == "creator" || currentElement == "language" || currentElement == "publisher" || currentElement == "date" || currentElement == "identifier" || currentElement == "description" || currentElement == "subject" {
            tempCharacters += string
        }
    }
    
    public func parser(_ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?, qualifiedName qName: String?) {
        let localName = elementName.components(separatedBy: ":").last ?? elementName
        let val = tempCharacters.trimmingCharacters(in: .whitespacesAndNewlines)
        
        if !val.isEmpty {
            switch localName {
            case "title":
                title = val
            case "creator":
                author = val
            case "language":
                language = val
            case "publisher":
                publisher = val
            case "date":
                date = val
            case "identifier":
                identifier = val
            case "description":
                bookDescription = val
            case "subject":
                subjects.append(val)
            default:
                break
            }
        }
        
        if currentElement == localName {
            currentElement = nil
        }
    }
}

// MARK: - Unified TOC Models

public final class TOCEntry: @unchecked Sendable, Equatable {
    public let title: String
    public let href: String
    public var children: [TOCEntry]
    
    public init(title: String, href: String, children: [TOCEntry] = []) {
        self.title = title
        self.href = href
        self.children = children
    }
    
    public static func == (lhs: TOCEntry, rhs: TOCEntry) -> Bool {
        lhs.title == rhs.title && lhs.href == rhs.href && lhs.children == rhs.children
    }
}

// MARK: - EPUB 3 Navigation TOC Parser

public final class EPUB3NavParser: NSObject, XMLParserDelegate {
    public var tocEntries: [TOCEntry] = []
    
    private var inTOCNav = false
    private var navDepth = 0
    private var currentHref: String?
    private var tempCharacters: String = ""
    
    private var entryStack: [TOCEntry] = []
    private var lastCreatedEntry: TOCEntry?
    
    public override init() {
        super.init()
    }
    
    public func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?, qualifiedName qName: String?, attributes attributeDict: [String : String] = [:]) {
        let localName = elementName.components(separatedBy: ":").last ?? elementName
        
        if inTOCNav {
            navDepth += 1
            if localName == "a" {
                currentHref = attributeDict["href"]
                tempCharacters = ""
            } else if localName == "ol" || localName == "ul" {
                if let last = lastCreatedEntry {
                    entryStack.append(last)
                    lastCreatedEntry = nil
                }
            }
        } else if localName == "nav" {
            let type = attributeDict["epub:type"] ?? attributeDict["type"]
            if type == "toc" {
                inTOCNav = true
                navDepth = 1
                entryStack = []
                lastCreatedEntry = nil
            }
        }
    }
    
    public func parser(_ parser: XMLParser, foundCharacters string: String) {
        if inTOCNav && currentHref != nil {
            tempCharacters += string
        }
    }
    
    public func parser(_ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?, qualifiedName qName: String?) {
        let localName = elementName.components(separatedBy: ":").last ?? elementName
        
        if inTOCNav {
            if localName == "a", let href = currentHref {
                let title = tempCharacters.trimmingCharacters(in: .whitespacesAndNewlines)
                if !title.isEmpty {
                    let entry = TOCEntry(title: title, href: href)
                    if let parent = entryStack.last {
                        parent.children.append(entry)
                    } else {
                        tocEntries.append(entry)
                    }
                    lastCreatedEntry = entry
                }
                currentHref = nil
            } else if localName == "ol" || localName == "ul" {
                if !entryStack.isEmpty {
                    _ = entryStack.removeLast()
                }
            }
            
            navDepth -= 1
            if navDepth == 0 {
                inTOCNav = false
            }
        }
    }
}

// MARK: - EPUB 2 NCX Parser

public final class EPUB2NCXParser: NSObject, XMLParserDelegate {
    public var tocEntries: [TOCEntry] = []
    
    private struct NavPointBuilder {
        var id: String
        var title: String = ""
        var href: String = ""
        var children: [TOCEntry] = []
    }
    
    private var builderStack: [NavPointBuilder] = []
    private var inText = false
    
    public override init() {
        super.init()
    }
    
    public func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?, qualifiedName qName: String?, attributes attributeDict: [String : String] = [:]) {
        let localName = elementName.components(separatedBy: ":").last ?? elementName
        
        if localName == "navPoint" {
            let id = attributeDict["id"] ?? ""
            builderStack.append(NavPointBuilder(id: id))
        } else if localName == "text" && !builderStack.isEmpty {
            inText = true
        } else if localName == "content" && !builderStack.isEmpty {
            builderStack[builderStack.count - 1].href = attributeDict["src"] ?? ""
        }
    }
    
    public func parser(_ parser: XMLParser, foundCharacters string: String) {
        if inText && !builderStack.isEmpty {
            builderStack[builderStack.count - 1].title += string
        }
    }
    
    public func parser(_ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?, qualifiedName qName: String?) {
        let localName = elementName.components(separatedBy: ":").last ?? elementName
        
        if localName == "text" {
            inText = false
        } else if localName == "navPoint" {
            if !builderStack.isEmpty {
                let completedBuilder = builderStack.removeLast()
                let title = completedBuilder.title.trimmingCharacters(in: .whitespacesAndNewlines)
                let entry = TOCEntry(title: title, href: completedBuilder.href, children: completedBuilder.children)
                
                if !builderStack.isEmpty {
                    builderStack[builderStack.count - 1].children.append(entry)
                } else {
                    tocEntries.append(entry)
                }
            }
        }
    }
}

// MARK: - XHTML Text Extraction Options

public struct EPUBTextExtractionOptions: Sendable {
    public var ignoredTags: Set<String>
    public var ignoredClasses: Set<String>
    
    public init(ignoredTags: Set<String>, ignoredClasses: Set<String>) {
        self.ignoredTags = ignoredTags
        self.ignoredClasses = ignoredClasses
    }
    
    public static let `default` = EPUBTextExtractionOptions(
        ignoredTags: ["head", "style", "script", "aside", "table", "footer", "nav"],
        ignoredClasses: ["footnote", "aside", "nav", "toc", "ad", "advertisement"]
    )
}

// MARK: - XHTML Text Extraction SAX Delegate

public final class EPUBTextExtractorDelegate: NSObject, XMLParserDelegate, @unchecked Sendable {
    private let options: EPUBTextExtractionOptions
    
    public var extractedText: String = ""
    private var ignoredTagsStack: [String] = []
    
    private static let blockElements: Set<String> = ["p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "li", "tr", "br", "aside", "table", "section", "article", "header", "footer", "nav"]
    
    public init(options: EPUBTextExtractionOptions) {
        self.options = options
        super.init()
    }
    
    public func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?, qualifiedName qName: String?, attributes attributeDict: [String : String] = [:]) {
        let localName = elementName.components(separatedBy: ":").last ?? elementName
        let lowerName = localName.lowercased()
        
        if Self.blockElements.contains(lowerName) {
            if !extractedText.isEmpty && !extractedText.hasSuffix("\n") {
                extractedText.append("\n")
            }
        }
        
        var shouldIgnore = options.ignoredTags.contains(lowerName)
        if !shouldIgnore, !options.ignoredClasses.isEmpty, let classesStr = attributeDict["class"] {
            let classes = classesStr.split(separator: " ").map(String.init)
            for cls in classes {
                if options.ignoredClasses.contains(cls) {
                    shouldIgnore = true
                    break
                }
            }
        }
        
        if shouldIgnore {
            ignoredTagsStack.append(lowerName)
        }
    }
    
    public func parser(_ parser: XMLParser, foundCharacters string: String) {
        if ignoredTagsStack.isEmpty {
            extractedText.append(string)
        }
    }
    
    public func parser(_ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?, qualifiedName qName: String?) {
        let localName = elementName.components(separatedBy: ":").last ?? elementName
        let lowerName = localName.lowercased()
        
        if let last = ignoredTagsStack.last, last == lowerName {
            _ = ignoredTagsStack.removeLast()
        }
        
        if Self.blockElements.contains(lowerName) {
            if !extractedText.isEmpty && !extractedText.hasSuffix("\n") {
                extractedText.append("\n")
            }
        }
    }
}

// MARK: - XHTML Text Extraction

/// Strips XHTML markup to clean, paragraph-collapsed plain text.
public enum EPUBTextExtractor {
    public static func plainText(
        fromXHTML xhtml: String,
        options: EPUBTextExtractionOptions = .default
    ) throws -> String {
        // Preprocess non-XML entities to prevent XMLParser from failing on them
        var preprocessed = xhtml
        let entitiesToReplace = [
            "&nbsp;": " ",
            "&ldquo;": "“",
            "&rdquo;": "”",
            "&lsquo;": "‘",
            "&rsquo;": "’",
            "&laquo;": "«",
            "&raquo;": "»",
            "&mdash;": "—",
            "&ndash;": "–",
            "&hellip;": "…"
        ]
        for (entity, unicode) in entitiesToReplace {
            preprocessed = preprocessed.replacingOccurrences(of: entity, with: unicode)
        }
        
        if let data = preprocessed.data(using: .utf8) {
            let parser = XMLParser(data: data)
            let delegate = EPUBTextExtractorDelegate(options: options)
            parser.delegate = delegate
            if parser.parse() && parser.parserError == nil {
                return delegate.extractedText
                    .components(separatedBy: .newlines)
                    .map { $0.trimmingCharacters(in: .whitespaces) }
                    .filter { !$0.isEmpty }
                    .joined(separator: "\n")
            }
        }
        
        // Fallback to raw character scanner
        return try plainTextFallback(fromXHTML: xhtml, options: options)
    }
    
    private static func plainTextFallback(
        fromXHTML xhtml: String,
        options: EPUBTextExtractionOptions
    ) throws -> String {
        var result = ""
        var inTag = false
        var tagBuffer = ""
        var activeIgnoredTagsStack: [String] = []
        
        let chars = Array(xhtml)
        var i = 0
        while i < chars.count {
            let char = chars[i]
            if char == "<" {
                inTag = true
                tagBuffer = ""
            } else if char == ">" {
                inTag = false
                let trimmedTag = tagBuffer.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
                
                if trimmedTag.hasPrefix("/") {
                    let endTagName = String(trimmedTag.dropFirst()).trimmingCharacters(in: .whitespaces)
                    if let last = activeIgnoredTagsStack.last, last == endTagName {
                        activeIgnoredTagsStack.removeLast()
                    }
                } else if !trimmedTag.hasSuffix("/") {
                    let (tagName, shouldIgnore) = shouldIgnoreTag(trimmedTag, options: options)
                    if shouldIgnore {
                        activeIgnoredTagsStack.append(tagName)
                    }
                }
            } else {
                if inTag {
                    tagBuffer.append(char)
                } else {
                    if activeIgnoredTagsStack.isEmpty {
                        result.append(char)
                    }
                }
            }
            i += 1
        }
        
        let decoded = decodeHTMLEntities(result)
        
        return decoded
            .components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
            .joined(separator: "\n")
    }
    
    private static func shouldIgnoreTag(
        _ tagContent: String,
        options: EPUBTextExtractionOptions
    ) -> (tagName: String, shouldIgnore: Bool) {
        let tokens = tagContent.split(separator: " ").map(String.init)
        guard let tagName = tokens.first else {
            return ("", false)
        }
        
        if options.ignoredTags.contains(tagName) {
            return (tagName, true)
        }
        
        if !options.ignoredClasses.isEmpty {
            // Find class="..." or class='...' using basic parsing
            if let classRange = tagContent.range(of: "class=") {
                let afterClass = tagContent[classRange.upperBound...]
                if let quoteChar = afterClass.first, (quoteChar == "\"" || quoteChar == "'") {
                    let rest = afterClass.dropFirst()
                    if let endQuoteIndex = rest.firstIndex(of: quoteChar) {
                        let classValue = String(rest[..<endQuoteIndex])
                        let classes = classValue.split(separator: " ").map(String.init)
                        for cls in classes {
                            if options.ignoredClasses.contains(cls) {
                                return (tagName, true)
                            }
                        }
                    }
                }
            }
        }
        
        return (tagName, false)
    }
    
    private static func decodeHTMLEntities(_ input: String) -> String {
        var result = input
        let entities = [
            "&amp;": "&",
            "&lt;": "<",
            "&gt;": ">",
            "&quot;": "\"",
            "&apos;": "'",
            "&nbsp;": " ",
            "&ldquo;": "“",
            "&rdquo;": "”",
            "&lsquo;": "‘",
            "&raquo;": "»",
            "&rsquo;": "’",
            "&laquo;": "«",
            "&mdash;": "—",
            "&ndash;": "–",
            "&hellip;": "…"
        ]
        for (entity, unicode) in entities {
            result = result.replacingOccurrences(of: entity, with: unicode)
        }
        
        // Resolve numeric entities like &#8212; or &#x2014;
        let pattern = "&#x?([0-9a-fA-F]+);"
        if let regex = try? NSRegularExpression(pattern: pattern, options: []) {
            let nsString = result as NSString
            let matches = regex.matches(in: result, options: [], range: NSRange(location: 0, length: nsString.length))
            for match in matches.reversed() {
                let range = match.range
                let capRange = match.range(at: 1)
                let capStr = nsString.substring(with: capRange)
                let isHex = nsString.substring(with: range).contains("x")
                let radix = isHex ? 16 : 10
                if let code = UInt32(capStr, radix: radix), let scalar = UnicodeScalar(code) {
                    result.replaceSubrange(Range(range, in: result)!, with: String(scalar))
                }
            }
        }
        
        return result
    }
}
