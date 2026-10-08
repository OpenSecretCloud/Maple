import Foundation
import Textual

struct PreparedMessage: Sendable {
    let revision: String
    let content: AttributedString
    let codeBlocks: [String: PreparedCode]
    let formulas: [MessageFormula]
    var equationSources: [String] = []
    var equationVersion = ""
    var presentationKey: String { "\(revision.utf8.count):" + revision + ":" + equationVersion }
}

struct PreparedCode: Sendable {
    let block: MessageCodeBlock
    let tokens: [MessageCodeToken]
}

actor MessageDocumentWorker: MessagePreparation {
    let parser = MessageMarkupParser()

    func prepare(_ source: String) async throws -> PreparedMessage {
        let markup = try await parser.parse(source)
        try Task.checkCancellation()
        guard !markup.isPlainText else {
            return .init(revision: source, content: AttributedString(source), codeBlocks: [:], formulas: [])
        }
        let parsed = try AttributedString(
            markdown: markup.markdown,
            options: .init(
                interpretedSyntax: .full, failurePolicy: .returnPartiallyParsedIfPossible,
                appliesSourcePositionAttributes: true))
        var content = try MessageMarkdownParity.normalize(parsed, source: markup.markdown)
        for run in Array(content.runs) {
            content[run.range].imageURL = nil
            content[run.range].textual.emojiURL = nil
            if let url = run.link,
                !["https", "http"].contains(url.scheme?.lowercased() ?? "") || url.host == nil || url.user != nil
                    || url.password != nil
            {
                content[run.range].link = nil
            }
        }
        // Restore literal streaming suffixes only after Markdown processing so
        // Foundation cannot consume TeX backslashes. Quoted/list code can contain
        // placeholders too; those are restored as code and never sent to KaTeX.
        for literal in markup.literals {
            if let range = content.range(of: literal.id) {
                var replacement = AttributedString(literal.source)
                replacement.presentationIntent = content[range].presentationIntent
                content.replaceSubrange(range, with: replacement)
            }
        }
        var formulas: [MessageFormula] = []
        for formula in markup.formulas {
            if let range = content.range(of: formula.id),
                content[range].presentationIntent?.components.contains(where: {
                    if case .codeBlock = $0.kind { return true }; return false
                }) == true
            {
                var replacement = AttributedString(formula.original)
                replacement.presentationIntent = content[range].presentationIntent
                content.replaceSubrange(range, with: replacement)
            } else {
                formulas.append(formula)
            }
        }
        // Assign quoted/nested fences an owned hint too, so no code path lazily
        // initializes the dependency's JS highlighter. Indented prose was normalized above.
        var ownedBlocks = markup.codeBlocks
        for run in Array(content.runs) {
            guard var intent = run.presentationIntent,
                let index = intent.components.firstIndex(where: {
                    if case .codeBlock = $0.kind { return true }
                    return false
                }),
                case .codeBlock(let hint) = intent.components[index].kind,
                hint.flatMap({ ownedBlocks[$0] }) == nil
            else { continue }
            let key = "maplenested\(intent.components[index].identity)"
            let existing = ownedBlocks[key]?.source ?? ""
            ownedBlocks[key] = .init(language: hint ?? "", source: existing + String(content[run.range].characters))
            intent.components[index].kind = .codeBlock(languageHint: key)
            content[run.range].presentationIntent = intent
        }
        let blocks = ownedBlocks.mapValues { block in
            PreparedCode(
                block: block, tokens: MessageCodeHighlighter.tokens(block.source, language: block.language))
        }
        try Task.checkCancellation()
        return .init(revision: source, content: content, codeBlocks: blocks, formulas: formulas)
    }

    func resolve(_ draft: PreparedMessage, equations: [String: AnyAttachment]) throws -> PreparedMessage {
        var content = draft.content
        for formula in draft.formulas {
            try Task.checkCancellation()
            if let range = content.range(of: formula.id) {
                var replacement = AttributedString(equations[formula.id] == nil ? formula.original : "\u{FFFC}")
                replacement.presentationIntent = content[range].presentationIntent
                replacement.inlinePresentationIntent = content[range].inlinePresentationIntent
                if let equation = equations[formula.id] { replacement.textual.attachment = equation }
                content.replaceSubrange(range, with: replacement)
            }
        }
        return .init(
            revision: draft.revision, content: content, codeBlocks: draft.codeBlocks, formulas: [],
            equationSources: draft.formulas.isEmpty ? draft.equationSources : draft.formulas.map(\.original),
            equationVersion: equations.keys.sorted().joined(separator: ","))
    }

}
