prod: ## Run scraper locally
	@. ./.env.sh && cargo run

log
	@cargo run --bin scraper | tee app.log
