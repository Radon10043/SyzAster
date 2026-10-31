# Makefile for SyzAster Technical Report
# Powered by ElegantPaper & MiKTeX (XeLaTeX + Biber)

XELATEX ?= xelatex
BIBER   ?= biber
PDFINFO ?= pdfinfo

MAIN    := report
SRC_TEX := $(MAIN).tex $(wildcard tables/*.tex)
SRC_BIB := refs.bib
SRC_FIG := $(patsubst %.drawio,%.pdf,$(wildcard images/*.drawio))
TARGET  := $(MAIN).pdf

LATEX_FLAGS := -interaction=nonstopmode -halt-on-error

.PHONY: all pdf figures clean distclean check

all: pdf check

pdf: $(TARGET)

$(TARGET): $(SRC_TEX) $(SRC_BIB) $(SRC_FIG)
	@echo "==> [1/4] Running initial XeLaTeX pass..."
	$(XELATEX) $(LATEX_FLAGS) $(MAIN).tex >/dev/null
	@echo "==> [2/4] Resolving bibliography references with Biber..."
	$(BIBER) $(MAIN) >/dev/null
	@echo "==> [3/4] Running second XeLaTeX pass..."
	$(XELATEX) $(LATEX_FLAGS) $(MAIN).tex >/dev/null
	@echo "==> [4/4] Finalizing cross-references and outlines..."
	$(XELATEX) $(LATEX_FLAGS) $(MAIN).tex >/dev/null
	@echo "==> Build successful: $(TARGET)"

# 用 draw.io 桌面版（无界面，经 xvfb）将 images/*.drawio 导出为裁边 PDF
DRAWIO ?= xvfb-run -a drawio --no-sandbox --disable-gpu --disable-dev-shm-usage
figures: $(SRC_FIG)

images/%.pdf: images/%.drawio
	$(DRAWIO) -x -f pdf --crop -o $@ $<

check: $(TARGET)
	@if command -v $(PDFINFO) >/dev/null 2>&1; then \
		PAGES=$$($(PDFINFO) $(TARGET) | awk '/^Pages:/ {print $$2}'); \
		echo "==> Generated PDF has $$PAGES pages."; \
		if [ "$$PAGES" -lt 6 ]; then \
			echo "WARNING: Page count ($$PAGES) is under the 6-page minimum requirement!"; \
			exit 1; \
		elif [ "$$PAGES" -gt 12 ]; then \
			echo "ERROR: Page count ($$PAGES) exceeds the 12-page maximum limit!"; \
			exit 1; \
		else \
			echo "==> Page count satisfies the competition rule (6-12 pages, target ~10 pages)."; \
		fi; \
	fi

clean:
	rm -f *.aux *.log *.out *.bbl *.bcf *.blg *.run.xml *.toc *.lot *.lof *.nav *.snm *.vrb

distclean: clean
	rm -f $(TARGET)
