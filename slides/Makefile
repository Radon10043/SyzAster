# Makefile for SyzAster Final Defense Slides
# Powered by Beamer (Metropolis) & MiKTeX (XeLaTeX)

XELATEX ?= xelatex
PDFINFO ?= pdfinfo

MAIN    := slides
SRC_TEX := $(MAIN).tex $(wildcard sections/*.tex) $(wildcard data/*.tex)
SRC_DAT := $(wildcard data/*.csv) $(wildcard images/*.png) $(wildcard images/*.jpg)
ARCH    := images/arch.pdf
DRAWIO  ?= xvfb-run -a drawio --no-sandbox --disable-gpu --disable-dev-shm-usage
TARGET  := $(MAIN).pdf

LATEX_FLAGS := -interaction=nonstopmode -halt-on-error

.PHONY: all pdf figures clean distclean check

all: pdf check

pdf: $(TARGET)

$(TARGET): $(SRC_TEX) $(SRC_DAT) $(ARCH)
	@echo "==> [1/2] Running initial XeLaTeX pass..."
	$(XELATEX) $(LATEX_FLAGS) $(MAIN).tex >/dev/null
	@echo "==> [2/2] Finalizing table of contents and page numbers..."
	$(XELATEX) $(LATEX_FLAGS) $(MAIN).tex >/dev/null
	@echo "==> Build successful: $(TARGET)"

# 幻灯片版架构图：在 report/images/arch.drawio 的基础上改用
# 黑体（Noto Sans CJK SC）+ 0xProto 字体，导出需要本机已安装 0xProto
figures: $(ARCH)

images/%.pdf: images/%.drawio
	$(DRAWIO) -x -f pdf --crop -o $@ $<

check: $(TARGET)
	@if command -v $(PDFINFO) >/dev/null 2>&1; then \
		PAGES=$$($(PDFINFO) $(TARGET) 2>/dev/null | awk '/^Pages:/ {print $$2}'); \
		echo "==> Generated PDF has $$PAGES pages."; \
	fi

clean:
	rm -f *.aux *.log *.out *.nav *.snm *.toc *.vrb *.xdv *.run.xml

distclean: clean
	rm -f $(TARGET)
