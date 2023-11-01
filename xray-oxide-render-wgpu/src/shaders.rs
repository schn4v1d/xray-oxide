use hassle_rs::{Dxc, DxcIncludeHandler, HassleError};
use std::path::Path;
use xray_oxide_core::filesystem::Filesystem;

pub struct ShaderModule {
    pub module: wgpu::ShaderModule,
    pub entry_point: String,
}

impl ShaderModule {
    pub fn create<P: AsRef<Path>, F: Fn(&str) -> (&str, &str)>(
        device: &wgpu::Device,
        filesystem: &Filesystem,
        shader_path: P,
        get_params: F,
    ) -> anyhow::Result<ShaderModule> {
        let mut path = filesystem.append_path("$game_shaders$", "r3").unwrap();
        path.push(shader_path.as_ref());

        let shader_code = filesystem.read_to_string(&path)?;

        let (target_profile, entry_point) = get_params(&shader_code);

        let dxc = Dxc::new(None)?;

        let compiler = dxc.create_compiler()?;
        let library = dxc.create_library()?;

        let blob = library.create_blob_with_encoding_from_str(&shader_code)?;

        let spirv = match compiler.compile(
            &blob,
            path.to_str().unwrap(),
            entry_point,
            target_profile,
            &["-spirv", "-Zs", "-Gec"],
            Some(&mut IncludeHandler { filesystem }),
            &[],
        ) {
            Err(result) => {
                let error_blob = result.0.get_error_buffer()?;
                Err(HassleError::CompileError(
                    library.get_blob_as_string(&error_blob.into())?,
                ))
            }
            Ok(result) => {
                let result_blob = result.get_result()?;

                Ok(result_blob.to_vec())
            }
        }?;

        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(
            path.with_file_name(path.file_name().unwrap().to_str().unwrap().to_owned() + ".spirv"),
            &spirv,
        )?;

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: shader_path.as_ref().to_str(),
            source: wgpu::util::make_spirv(&spirv),
        });

        Ok(ShaderModule {
            module,
            entry_point: entry_point.to_owned(),
        })
    }

    pub fn create_vertex<P: AsRef<Path>>(
        device: &wgpu::Device,
        filesystem: &Filesystem,
        shader_path: P,
    ) -> anyhow::Result<ShaderModule> {
        ShaderModule::create(
            device,
            filesystem,
            shader_path.as_ref().with_extension("vs"),
            |code| {
                let entry = if code.contains("main_vs_1_1") {
                    "main_vs_1_1"
                } else if code.contains("main_vs_2_0") {
                    "main_vs_2_0"
                } else {
                    "main"
                };

                ("vs_5_0", entry)
            },
        )
    }

    pub fn create_fragment<P: AsRef<Path>>(
        device: &wgpu::Device,
        filesystem: &Filesystem,
        shader_path: P,
    ) -> anyhow::Result<ShaderModule> {
        ShaderModule::create(
            device,
            filesystem,
            shader_path.as_ref().with_extension("ps"),
            |code| {
                let entry = if code.contains("main_ps_1_1") {
                    "main_ps_1_1"
                } else if code.contains("main_ps_1_2") {
                    "main_ps_1_2"
                } else if code.contains("main_ps_1_3") {
                    "main_ps_1_3"
                } else if code.contains("main_ps_1_4") {
                    "main_ps_1_4"
                } else if code.contains("main_ps_2_0") {
                    "main_ps_2_0"
                } else {
                    "main"
                };

                ("ps_5_0", entry)
            },
        )
    }
}

struct IncludeHandler<'a> {
    filesystem: &'a Filesystem,
}

impl<'a> DxcIncludeHandler for IncludeHandler<'a> {
    fn load_source(&mut self, filename: String) -> Option<String> {
        let filename = if cfg!(windows) {
            filename.replace('/', "\\")
        } else {
            filename
        };

        self.filesystem.read_to_string(filename).ok()
    }
}
