#if DEBUG
    import Foundation

    /// Shared synthetic source for native runtime fidelity and parser producer tests.
    enum RendererAcceptanceCorpus {
        static let markdown = #"""
            Soft first
            Soft second

            Hard first  
            Hard second

            # GFM heading one
            ## GFM heading two
            ### GFM heading three

            **Bold**, *italic*, ~~struck~~ and `inline code`.

            - unordered one
              - nested unordered child
            - [ ] Pending task marker
            - [x] Completed task marker

            3. ordered third
            4. ordered fourth

            > Quoted **rich text**.
            > Quote final.

                Indented ordinary prose in the Research baseline.

            <span>Raw HTML stays untrusted data.</span>

            Bare link https://example.test/autolink and [reference link][destination].

            [destination]: https://example.test/reference

            ![Preserved remote image alt](https://example.test/never.png)

            ```swift
            let synthetic = "copy code" // preserved
                print(synthetic)
            ```

            العربية עברית 中文 👩🏽‍💻 é.

            \[\begin{bmatrix}a & b\\c & d\end{bmatrix}\]

            \[f(x)=\begin{cases}x^2 & x\ge0\\-x & x<0\end{cases}\]

            Invalid original source \(\unsupportedcommand{x}\).
            Incomplete original source \(\frac{1}{
            """#
    }
#endif
