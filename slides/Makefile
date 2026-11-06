# Makefile for SyzAster Final Defense Slides
# Powered by Beamer (Metropolis) & MiKTeX (XeLaTeX)

XELATEX ?= xelatex
PDFINFO ?= pdfinfo

MAIN    := slides
SRC_TEX := $(MAIN).tex $(wildcard sections/*.tex) $(wildcard data/*.tex)
SRC_DAT := $(wildcard data/*.csv) $(wildcard images/*.png) $(wildcard images/*.jpg)
ARCH    := ../report/images/arch.pdf
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

# 架构图与技术报告共用同一份 draw.io 源文件，由 report/Makefile 导出
figures: $(ARCH)

$(ARCH): ../report/images/arch.drawio
	$(MAKE) -C ../report figures

check: $(TARGET)
	@if command -v $(PDFINFO) >/dev/null 2>&1; then \
		PAGES=$$($(PDFINFO) $(TARGET) 2>/dev/null | awk '/^Pages:/ {print $$2}'); \
		echo "==> Generated PDF has $$PAGES pages."; \
	fi

clean:
	rm -f *.aux *.log *.out *.nav *.snm *.toc *.vrb *.xdv *.run.xml

distclean: clean
	rm -f $(TARGET)
