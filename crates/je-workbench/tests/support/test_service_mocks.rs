//! Os `mock<ITestService>()` e `mock<ITestProfileService>()` dos testes do contrib testing: cada um implementa só o que o
//! caso sobrescreve, e todo outro membro lança `X is not a function`, como chamar um membro ausente no JavaScript.
//! Os comportamentos sobrescritos entram por fechos nos campos públicos.
#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::{Error, throw};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::position::Position;
use je_workbench::contrib::testing::common::observable_value::MutableObservableValue;
use je_workbench::contrib::testing::common::test_exclusions::TestExclusions;
use je_workbench::contrib::testing::common::test_profile_service::{ControllerProfiles, ITestProfileService};
use je_workbench::contrib::testing::common::test_result::ITestResult;
use je_workbench::contrib::testing::common::test_service::{
    AmbiguousRunTestsRequest, CancelTestRun, IMainThreadTestCollection, Location, IMainThreadTestController, IMainThreadTestHostProxy, ITestFollowups, ITestService,
};
use je_workbench::contrib::testing::common::test_types::{
    ITestItem, ITestRunProfile, InternalTestItem, PartialTestRunProfile, ResolvedTestRunRequest, TestMessageFollowupRequest, TestRunProfileBitset, TestsDiff,
};

type StartContinuousRun = Box<dyn Fn(ResolvedTestRunRequest, CancellationToken) -> Promise<()>>;

/// `class MockTestService extends mock<ITestService>()`: só o `startContinuousRun` é sobrescrito.
#[derive(Default)]
pub struct MockTestService {
    pub start_continuous_run: RefCell<Option<StartContinuousRun>>,
}

impl ITestService for MockTestService {
    fn on_did_cancel_test_run(&self) -> Event<CancelTestRun> {
        throw(Error::new("onDidCancelTestRun is not a function"))
    }
    fn excluded(&self) -> Rc<TestExclusions> {
        throw(Error::new("excluded is not a function"))
    }
    fn collection(&self) -> Rc<dyn IMainThreadTestCollection> {
        throw(Error::new("collection is not a function"))
    }
    fn on_will_process_diff(&self) -> Event<Rc<TestsDiff>> {
        throw(Error::new("onWillProcessDiff is not a function"))
    }
    fn on_did_process_diff(&self) -> Event<Rc<TestsDiff>> {
        throw(Error::new("onDidProcessDiff is not a function"))
    }
    fn show_inline_output(&self) -> Rc<MutableObservableValue<bool>> {
        throw(Error::new("showInlineOutput is not a function"))
    }
    fn register_ext_host(&self, controller: Rc<dyn IMainThreadTestHostProxy>) -> DisposableRef {
        throw(Error::new("registerExtHost is not a function"))
    }
    fn register_test_controller(&self, provider_id: &str, controller: Rc<dyn IMainThreadTestController>) -> DisposableRef {
        throw(Error::new("registerTestController is not a function"))
    }
    fn get_test_controller(&self, controller_id: &str) -> Option<Rc<dyn IMainThreadTestController>> {
        throw(Error::new("getTestController is not a function"))
    }
    fn get_test_controllers(&self) -> Vec<Rc<dyn IMainThreadTestController>> {
        throw(Error::new("getTestControllers is not a function"))
    }
    fn refresh_tests(&self, controller_id: Option<&str>) -> Promise<()> {
        throw(Error::new("refreshTests is not a function"))
    }
    fn cancel_refresh_tests(&self) {
        throw(Error::new("cancelRefreshTests is not a function"))
    }
    fn start_continuous_run(&self, req: ResolvedTestRunRequest, token: CancellationToken) -> Promise<()> {
        match self.start_continuous_run.borrow().as_ref() {
            Some(start) => start(req, token),
            None => throw(Error::new("startContinuousRun is not a function")),
        }
    }
    fn run_tests(&self, req: AmbiguousRunTestsRequest, token: Option<CancellationToken>) -> Promise<Rc<dyn ITestResult>> {
        throw(Error::new("runTests is not a function"))
    }
    fn run_resolved_tests(&self, req: ResolvedTestRunRequest, token: Option<CancellationToken>) -> Promise<Rc<dyn ITestResult>> {
        throw(Error::new("runResolvedTests is not a function"))
    }
    fn provide_test_followups(&self, req: TestMessageFollowupRequest, token: CancellationToken) -> Promise<Rc<dyn ITestFollowups>> {
        throw(Error::new("provideTestFollowups is not a function"))
    }
    fn sync_tests(&self) -> Promise<()> {
        throw(Error::new("syncTests is not a function"))
    }
    fn cancel_test_run(&self, run_id: Option<&str>, task_id: Option<&str>) {
        throw(Error::new("cancelTestRun is not a function"))
    }
    fn publish_diff(&self, controller_id: &str, diff: TestsDiff) {
        throw(Error::new("publishDiff is not a function"))
    }
    fn get_tests_related_to_code(&self, uri: &URI, position: Position, token: Option<CancellationToken>) -> Promise<Vec<InternalTestItem>> {
        throw(Error::new("getTestsRelatedToCode is not a function"))
    }
    fn get_code_related_to_test(&self, test: InternalTestItem, token: Option<CancellationToken>) -> Promise<Vec<Location>> {
        throw(Error::new("getCodeRelatedToTest is not a function"))
    }
}

type GroupDefaults = Box<dyn Fn(TestRunProfileBitset, Option<&str>) -> Vec<ITestRunProfile>>;

/// `class MockProfilesService extends mock<ITestProfileService>()`: `onDidChange` e `getGroupDefaultProfiles`.
pub struct MockProfilesService {
    pub did_change_emitter: Rc<Emitter<()>>,
    pub get_group_default_profiles: RefCell<GroupDefaults>,
}

impl MockProfilesService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { did_change_emitter: Emitter::new(), get_group_default_profiles: RefCell::new(Box::new(|_, _| Vec::new())) })
    }
}

impl IDisposable for MockProfilesService {
    fn dispose(&self) {
        self.did_change_emitter.dispose();
    }
}

impl ITestProfileService for MockProfilesService {
    fn on_did_change(&self) -> Event<()> {
        self.did_change_emitter.event()
    }
    fn add_profile(&self, controller: Rc<dyn IMainThreadTestController>, profile: ITestRunProfile) {
        throw(Error::new("addProfile is not a function"))
    }
    fn update_profile(&self, controller_id: &str, profile_id: u32, update: PartialTestRunProfile) {
        throw(Error::new("updateProfile is not a function"))
    }
    fn remove_profile(&self, controller_id: &str, profile_id: Option<u32>) {
        throw(Error::new("removeProfile is not a function"))
    }
    fn capabilities_for_test(&self, test: &ITestItem) -> u32 {
        throw(Error::new("capabilitiesForTest is not a function"))
    }
    fn configure(&self, controller_id: &str, profile_id: u32) {
        throw(Error::new("configure is not a function"))
    }
    fn all(&self) -> Vec<ControllerProfiles> {
        throw(Error::new("all is not a function"))
    }
    fn get_group_default_profiles(&self, group: TestRunProfileBitset, controller_id: Option<&str>) -> Vec<ITestRunProfile> {
        (self.get_group_default_profiles.borrow())(group, controller_id)
    }
    fn set_group_default_profiles(&self, group: TestRunProfileBitset, profiles: &[ITestRunProfile]) {
        throw(Error::new("setGroupDefaultProfiles is not a function"))
    }
    fn get_controller_profiles(&self, controller_id: &str) -> Vec<ITestRunProfile> {
        throw(Error::new("getControllerProfiles is not a function"))
    }
    fn get_default_profile_for_test(&self, group: TestRunProfileBitset, test: &InternalTestItem) -> Option<ITestRunProfile> {
        throw(Error::new("getDefaultProfileForTest is not a function"))
    }
}
