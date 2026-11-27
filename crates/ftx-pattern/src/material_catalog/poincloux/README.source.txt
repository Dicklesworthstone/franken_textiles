clamped prefix: Experimental data of a 51 x 51 (column x row) stockinette stitch fabric under displacement-imposed cyclic uniaxial tension, with the top and bottom row clamped with imposed lateral stitch size. There are four files:\

- clamped_force_data.txt: tab-separated columns containing the time series of the force response. Each line of the corresponding columns features: the experimental time (seconds), the total vertical size of the fabric (in millimeters), and the force response in tension (in Newtons). 

- clamped_picture_info.txt: tab-separated columns containing the time series of information about the pictures taken to measure the stitches' position field. Each line of the corresponding columns features: the index of the picture, the time when the picture is taken (in seconds, synchronized with the force response), and the total vertical size of the fabric (in millimeters). 

Properties of the stitches extracted from image analysis are reported in the form of matrices, where each element corresponds to the property of one stitch. A rectangular patch of [43 x 36] stitches has been successfully tracked, with 43 stitches in the column/wale direction, and 36 in the line/course direction. The stitch in [1 , 1] corresponds to the one at the bottom left of the fabric.  

- clamped_stitch_pos_ref.mat (and .h5): this file contains a [43 x 36 x 4] matrix. The 2D matrices stitch_pos_ref( : , : , 1 ) and stitch_pos_ref( : , : , 2 ) contain, respectively, the column and row indices of the stitches (the entire fabric contains 51 stitch columns and 51 stitch lines, but only a subset of 43 rows and 36 columns is successfully tracked). The matrices stitch_pos_ref( : , : , 3 ) and stitch_pos_ref( : , : , 4 ) contain respectively the x (horizontal) and y (vertical) position of the stitches in the absolute reference configuration, where all columns are separated by c*=3.93 mm and rows by w*=2.08 mm (see article for details). 

- clamped_stitch_posx.mat (and .h5): this file contains a [43 x 36 x 150] matrix containing the horizontal (x) positions of every stitch and at each picture timestep. For instance, stitch_posx( : , : , 15 ) contains the x positions for the picture of index 15 (see file picture_info.txt). 

- clamped_stitch_posy.mat (and .h5): same as for stitch_posx but for the vertical (y) positions.

%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%%

sliding prefix: Experimental data of a 20 x 30 (column x row) stockinette stitch fabric under displacement-imposed cyclic uniaxial tension, with the top and bottom row that can freely slide laterally. There are three files:

- sliding_force_data.txt: tab-separated columns containing the time series of the force response. Each line of the corresponding columns features: the experimental time (seconds), the total vertical size of the fabric (in millimeters), and the force response in tension (in Newtons). 

- sliding_pictures.zip: contains the raw picture of the fabric under test. 

- sliding_picture_info.txt: tab-separated columns containing the time series of information about the pictures taken to measure the stitches' position field. Each line of the corresponding columns features: the index of the picture (number in the picture's name), the time when the picture is taken (in seconds, synchronized with the force response), and the total vertical size of the fabric (in millimeters).

