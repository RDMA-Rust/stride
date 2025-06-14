use crate::cli::plan::Plan;
use crate::runners::runner::PlanTestRunner;
use anyhow::Result;

pub fn run_worker(plan: Plan) -> Result<()> {
    let runner = PlanTestRunner::new(plan);
    runner.run().map_err(|e| anyhow::anyhow!("{}", e))
}
