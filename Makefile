prod: ## Run scraper locally
	@. ./.env.sh && cargo run

log
	@cargo run --bin scraper 2>&1 | tee app.log
