ARCH_DOCS   := architecture.md $(wildcard architecture/*.md)
OUTPUT      := architecture.pdf

.PHONY: pdf clean

# Build PDF using local pandoc/xelatex
pdf: $(OUTPUT)

$(OUTPUT): $(ARCH_DOCS)
	pandoc $(ARCH_DOCS) \
		--pdf-engine=xelatex \
		-V geometry:margin=1in \
		-V fontsize=11pt \
		--toc \
		--toc-depth=2 \
		-o $@

clean:
	rm -f $(OUTPUT)
