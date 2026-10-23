//! Sonda H01: quais backends e adapters o wgpu enxerga nesta máquina (WSLg).

fn main() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::all();
    let instance = wgpu::Instance::new(descriptor);
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    println!("{} adapters", adapters.len());
    for a in &adapters {
        let i = a.get_info();
        println!(
            "{:?} | {} | {:?} | driver={} {}",
            i.backend, i.name, i.device_type, i.driver, i.driver_info
        );
    }
    let pick = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }));
    match pick {
        Ok(a) => println!("escolhido: {:?} {}", a.get_info().backend, a.get_info().name),
        Err(e) => println!("nenhum adapter: {e}"),
    }
}
