<script lang="ts">
    import { marked } from 'marked';
    import { highlightCode } from './highlighter';
    import { slugify } from './types';
    import './markdown.css';

    interface Props {
        content: string;
    }

    let { content }: Props = $props();

    let parsedHtml = $derived.by(() => {
        try {
            // Configure marked with GFM enabled, no breaks
            marked.setOptions({
                gfm: true,
                breaks: false,
            });

            // Set up custom renderer for code blocks to enable syntax highlighting
            const renderer = new marked.Renderer();

            renderer.heading = (token) => {
                const id = slugify(token.text);
                const level = token.depth;
                return `<h${level} id="${id}" class="anchor-heading"><a class="anchor-link" href="#${id}">#</a>${token.text}</h${level}>`;
            };

            renderer.codespan = (token) => {
                return `<code class="inline-code">${token.text}</code>`;
            };

            renderer.code = (token) => {
                const language = token.lang || undefined;
                const highlightedCode = highlightCode(token.text, language);
                const languageClass = language ? ` language-${language}` : '';
                return `<pre><code class="hljs${languageClass}">${highlightedCode}</code></pre>`;
            };

            marked.setOptions({ renderer });

            return marked.parse(content);
        } catch (error) {
            console.error('Markdown parsing error:', error);
            return `<p>Error rendering markdown: ${error instanceof Error ? error.message : String(error)}</p>`;
        }
    });
</script>

<div class="markdown-renderer">
    {@html parsedHtml}
</div>

<style>
    .markdown-renderer {
        padding: 32px;
        font-size: 15px;
        line-height: 1.7;
        color: var(--color-fg-alt, #666);
    }
</style>
