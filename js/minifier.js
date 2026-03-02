// js/minifier.js
// Port of bX\Scon\Minifier — SCON minification and expansion

export class Minifier {

    // Minify SCON to single line
    // Rules:
    //   ; = newline (same depth)
    //   N semicolons (N>=2) = dedent (N-1) levels
    //   ;; = dedent 1, ;;; = dedent 2, ;;;; = dedent 3, etc.
    //   Strings with ; must be quoted (\; escape)
    static minify(scon) {
        const lines = scon.split('\n');
        let result = '';
        let prevDepth = 0;
        let isFirst = true;

        // Auto-detect indent from first indented line
        let indent = 1;
        const indentMatch = scon.match(/\n( +)\S/);
        if (indentMatch) indent = indentMatch[1].length;

        for (const line of lines) {
            const trimmed = line.trim();

            // Skip empty lines and comments
            if (trimmed === '' || trimmed[0] === '#') {
                // Preserve header
                if (trimmed.startsWith('#!scon/')) {
                    result += trimmed + ';';
                }
                continue;
            }

            const depth = Minifier._calculateDepth(line, indent);

            if (!isFirst) {
                const diff = prevDepth - depth;
                if (diff >= 2) {
                    result += ';'.repeat(diff + 1);
                } else if (diff === 1) {
                    result += ';;';
                } else {
                    result += ';';
                }
            }

            result += trimmed;
            prevDepth = depth;

            // Scope openers: increase expected depth for children
            if (/:$/.test(trimmed)) {
                prevDepth = depth + 1;
            }
            // List item (- key: val) children are indented 1 level deeper
            if (/^- /.test(trimmed)) {
                prevDepth = depth + 1;
            }

            isFirst = false;
        }

        return result;
    }

    // Expand minified SCON to indented format
    static expand(minified, indent = 1) {
        const lines = [];
        let depth = 0;
        let buffer = '';
        let inQuotes = false;
        const len = minified.length;

        for (let i = 0; i < len; i++) {
            const char = minified[i];

            // Handle escape in quotes
            if (char === '\\' && inQuotes && i + 1 < len) {
                buffer += char + minified[i + 1];
                i++;
                continue;
            }

            if (char === '"') {
                inQuotes = !inQuotes;
                buffer += char;
                continue;
            }

            if (char === ';' && !inQuotes) {
                // Count consecutive semicolons
                let semiCount = 1;
                while (i + 1 < len && minified[i + 1] === ';') {
                    semiCount++;
                    i++;
                }

                // Emit current buffer
                const trimmed = buffer.trim();
                if (trimmed !== '') {
                    lines.push(' '.repeat(indent * depth) + trimmed);

                    // Scope openers: increase depth for children
                    if (/:$/.test(trimmed) && !/:\s*\S/.test(trimmed)) {
                        depth++;
                    }
                    // List item children are indented 1 level deeper
                    if (/^- /.test(trimmed)) {
                        depth++;
                    }
                }

                buffer = '';

                // Apply dedent: N semicolons = dedent (N-1) levels
                if (semiCount >= 2) {
                    depth = Math.max(0, depth - (semiCount - 1));
                }

                continue;
            }

            buffer += char;
        }

        // Last buffer
        const trimmed = buffer.trim();
        if (trimmed !== '') {
            lines.push(' '.repeat(indent * depth) + trimmed);
        }

        return lines.join('\n');
    }

    static _calculateDepth(line, indent = 1) {
        let spaces = 0;
        for (let i = 0; i < line.length; i++) {
            if (line[i] === ' ') spaces++;
            else break;
        }
        return indent > 0 ? Math.floor(spaces / indent) : 0;
    }
}
